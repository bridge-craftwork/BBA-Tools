#!/usr/bin/env python3
"""
Comprehensive BBA server restart script.
Checks if running, shuts down gracefully, waits for shutdown, starts up,
monitors for startup evidence, then runs health and test calls.

Also manages the Cloudflare tunnel for public access.

Usage:
    python3 restart-server.py           # Just restart
    python3 restart-server.py --rebuild # Rebuild then restart
"""
import sys
import os
import time
import json
import subprocess
import argparse
from datetime import datetime

# Add the PBS build-scripts-mac to path
PBS_BUILD_SCRIPTS = os.path.expanduser(
    "/Volumes/Express2T/Development/GitHub/Practice-Bidding-Scenarios/build-scripts-mac"
)
sys.path.insert(0, PBS_BUILD_SCRIPTS)

from ssh_runner import run_windows_command, test_ssh_connection

# Configuration
WINDOWS_SERVER_PATH = r"G:\BBA-Tools\bba-server"
WINDOWS_LOG_PATH = r"G:\BBA-Tools\bba-server\bin\Debug\net8.0-windows\logs"
SERVER_PORT = 5000
VM_IP = "10.211.55.5"
SHUTDOWN_TIMEOUT = 15  # seconds to wait for shutdown
STARTUP_TIMEOUT = 30   # seconds to wait for startup

# Cloudflare tunnel configuration
CLOUDFLARED_VBS = r"C:\Users\rick\.cloudflared\start-tunnel.vbs"
PUBLIC_URL = "https://bba.harmonicsystems.com"

# Test deal for verification
TEST_DEAL = {
    "deal": {
        "pbn": "N:A653.Q97.K64.954 KQ4.AT8432.A72.A J8.K.QJ98.KJT762 T972.J65.T53.Q83",
        "dealer": "S",
        "vulnerability": "NS",
        "scoring": "MP"
    },
    "scenario": "Smolen"
}


def log(msg: str):
    """Print timestamped message."""
    print(f"[{datetime.now().strftime('%H:%M:%S')}] {msg}")


def is_server_running() -> bool:
    """Check if server is listening on the port."""
    cmd = f'netstat -ano | findstr ":{SERVER_PORT}" | findstr "LISTENING"'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)
    return returncode == 0 and stdout.strip() != ""


def get_dotnet_pid() -> str | None:
    """Get PID of dotnet process listening on server port."""
    cmd = f'netstat -ano | findstr ":{SERVER_PORT}" | findstr "LISTENING"'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)
    if returncode == 0 and stdout.strip():
        # Parse PID from netstat output (last column)
        lines = stdout.strip().split('\n')
        for line in lines:
            parts = line.split()
            if parts:
                return parts[-1]
    return None


def shutdown_server() -> bool:
    """Send shutdown signal to server via health endpoint or kill process."""
    log("Sending shutdown signal...")

    # Try graceful shutdown via taskkill with PID
    pid = get_dotnet_pid()
    if pid:
        log(f"  Found server process PID: {pid}")
        cmd = f'taskkill /PID {pid} /F'
        returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)
        if returncode == 0:
            log("  Shutdown signal sent")
            return True
        else:
            log(f"  taskkill failed: {stderr}")

    # Fallback: kill all dotnet processes
    log("  Falling back to killing all dotnet processes")
    cmd = 'taskkill /f /im dotnet.exe 2>nul'
    run_windows_command(cmd, check=False, verbose=False)
    return True


def wait_for_shutdown() -> bool:
    """Wait for server to fully shut down."""
    log(f"Waiting for shutdown (timeout: {SHUTDOWN_TIMEOUT}s)...")

    for i in range(SHUTDOWN_TIMEOUT):
        if not is_server_running():
            log(f"  Server stopped after {i+1}s")
            return True
        time.sleep(1)
        print(".", end="", flush=True)

    print()
    log("  Shutdown timeout reached")
    return False


def start_server_background() -> bool:
    """Start server in background using schtasks to create a one-time task."""
    log("Starting server in background...")

    # Path to the built executable (net8.0-windows for Windows target)
    exe_path = r"G:\BBA-Tools\bba-server\bin\Debug\net8.0-windows\bba-server.exe"
    working_dir = r"G:\BBA-Tools\bba-server\bin\Debug\net8.0-windows"

    # Method 1: Use schtasks to create and run a one-time task
    # This detaches from the SSH session properly
    task_name = "BBAServerStart"

    # Delete any existing task
    run_windows_command(f'schtasks /delete /tn {task_name} /f 2>nul', check=False, verbose=False)

    # Use pre-created VBS script to run the server hidden
    # The VBS file was created once and lives in the bin directory
    vbs_path = r"G:\BBA-Tools\bba-server\bin\Debug\net8.0-windows\start-hidden.vbs"

    # Create a task to run the VBS script
    create_cmd = f'schtasks /create /tn {task_name} /tr "wscript.exe \\"{vbs_path}\\"" /sc once /st 00:00 /f'

    returncode, stdout, stderr = run_windows_command(create_cmd, check=False, verbose=False)
    if returncode != 0:
        log(f"  Failed to create scheduled task: {stderr}")
        return False

    # Run the task immediately
    run_cmd = f'schtasks /run /tn {task_name}'
    returncode, stdout, stderr = run_windows_command(run_cmd, check=False, verbose=False)

    if returncode == 0:
        log("  Server start task triggered")
        # Clean up the task after a delay (it will have started by then)
        time.sleep(2)
        run_windows_command(f'schtasks /delete /tn {task_name} /f 2>nul', check=False, verbose=False)
        return True
    else:
        log(f"  Failed to run scheduled task: {stderr}")
        return False


def get_latest_log_content(lines: int = 20) -> str:
    """Get last N lines of the most recent log file."""
    # Find today's log file
    today = datetime.now().strftime("%Y-%m-%d")
    log_pattern = f"log-{today}*.txt"

    cmd = f'cd /d {WINDOWS_LOG_PATH} && dir /b /o-d log-*.txt 2>nul'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)

    if returncode != 0 or not stdout.strip():
        return ""

    # Get the most recent log file
    log_file = stdout.strip().split('\n')[0]
    log_full_path = f"{WINDOWS_LOG_PATH}\\{log_file}"

    # Get last N lines
    cmd = f'powershell -Command "Get-Content \'{log_full_path}\' -Tail {lines}"'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)

    return stdout if returncode == 0 else ""


def wait_for_startup() -> bool:
    """Wait for server to start up by monitoring log and port."""
    log(f"Waiting for startup (timeout: {STARTUP_TIMEOUT}s)...")

    for i in range(STARTUP_TIMEOUT):
        # Check if port is listening
        if is_server_running():
            log(f"  Server listening after {i+1}s")

            # Also check log for startup message
            log_content = get_latest_log_content(10)
            if "Now listening on" in log_content or "Application started" in log_content:
                log("  Startup confirmed in log")

            return True

        time.sleep(1)
        print(".", end="", flush=True)

    print()
    log("  Startup timeout reached")
    return False


def health_check() -> bool:
    """Call health endpoint and verify response."""
    log("Running health check...")

    try:
        result = subprocess.run(
            ["curl", "-s", "-w", "\\n%{http_code}", f"http://{VM_IP}:{SERVER_PORT}/health"],
            capture_output=True,
            text=True,
            timeout=10
        )

        lines = result.stdout.strip().split('\n')
        if len(lines) >= 2:
            status_code = lines[-1]
            body = '\n'.join(lines[:-1])

            if status_code == "200":
                log(f"  Health check passed: {body}")
                return True
            else:
                log(f"  Health check failed: HTTP {status_code}")
                return False
        else:
            log(f"  Health check failed: unexpected response")
            return False

    except subprocess.TimeoutExpired:
        log("  Health check timeout")
        return False
    except Exception as e:
        log(f"  Health check error: {e}")
        return False


def test_auction_call() -> bool:
    """Run a test auction generation call."""
    log("Running test auction call...")

    try:
        result = subprocess.run(
            ["curl", "-s", "-X", "POST",
             "-H", "Content-Type: application/json",
             "-d", json.dumps(TEST_DEAL),
             f"http://{VM_IP}:{SERVER_PORT}/api/auction/generate"],
            capture_output=True,
            text=True,
            timeout=30
        )

        if result.returncode == 0:
            try:
                response = json.loads(result.stdout)
                if response.get("success"):
                    auction = response.get("auction", [])
                    log(f"  Test call succeeded: {' '.join(auction)}")
                    return True
                else:
                    log(f"  Test call failed: {response.get('error')}")
                    return False
            except json.JSONDecodeError:
                log(f"  Test call failed: invalid JSON response")
                return False
        else:
            log(f"  Test call failed: curl error")
            return False

    except subprocess.TimeoutExpired:
        log("  Test call timeout")
        return False
    except Exception as e:
        log(f"  Test call error: {e}")
        return False


def show_recent_log():
    """Display recent log entries."""
    log("Recent log entries:")
    log_content = get_latest_log_content(15)
    if log_content:
        for line in log_content.split('\n'):
            if line.strip():
                print(f"    {line}")
    else:
        print("    (no log entries found)")


def build_server() -> bool:
    """Build the server project using dotnet build."""
    log("Building bba-server...")

    cmd = f'cd /d {WINDOWS_SERVER_PATH} && dotnet build -c Debug'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)

    if returncode == 0:
        # Check for build success in output
        if "Build succeeded" in stdout:
            log("  Build succeeded")
            return True
        elif "0 Error(s)" in stdout:
            log("  Build succeeded")
            return True
        else:
            log("  Build completed (check output for warnings)")
            return True
    else:
        log("  Build FAILED!")
        # Show build output for debugging
        if stdout:
            for line in stdout.split('\n'):
                if 'error' in line.lower() or 'failed' in line.lower():
                    print(f"    {line}")
        if stderr:
            print(f"    stderr: {stderr}")
        return False


def is_cloudflared_running() -> bool:
    """Check if cloudflared is running."""
    cmd = 'tasklist | findstr cloudflared'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)
    return returncode == 0 and "cloudflared" in stdout


def start_cloudflared() -> bool:
    """Start cloudflared tunnel using VBS script."""
    log("Starting Cloudflare tunnel...")

    cmd = f'wscript.exe "{CLOUDFLARED_VBS}"'
    returncode, _, stderr = run_windows_command(cmd, check=False, verbose=False)

    if returncode != 0:
        log(f"  Failed to start tunnel: {stderr}")
        return False

    # Wait for it to connect
    time.sleep(5)

    if is_cloudflared_running():
        log("  Cloudflare tunnel started")
        return True
    else:
        log("  Cloudflare tunnel may not have started")
        return False


def check_public_endpoint() -> bool:
    """Check if the public endpoint is accessible."""
    log(f"Checking public endpoint ({PUBLIC_URL})...")

    try:
        result = subprocess.run(
            ["curl", "-s", "-w", "\\n%{http_code}", f"{PUBLIC_URL}/health"],
            capture_output=True,
            text=True,
            timeout=15
        )

        lines = result.stdout.strip().split('\n')
        if len(lines) >= 2:
            status_code = lines[-1]
            if status_code == "200":
                log(f"  Public endpoint OK")
                return True
            else:
                log(f"  Public endpoint returned HTTP {status_code}")
                return False
        else:
            log(f"  Public endpoint check failed: unexpected response")
            return False

    except subprocess.TimeoutExpired:
        log("  Public endpoint timeout")
        return False
    except Exception as e:
        log(f"  Public endpoint error: {e}")
        return False


def main():
    parser = argparse.ArgumentParser(description="BBA Server restart script")
    parser.add_argument("--rebuild", action="store_true",
                        help="Rebuild the server before restarting")
    args = parser.parse_args()

    print("=" * 60)
    if args.rebuild:
        print("BBA Server Rebuild & Restart Script")
    else:
        print("BBA Server Restart Script")
    print("=" * 60)
    print()

    # Test SSH connection
    log("Testing SSH connection to Windows VM...")
    if not test_ssh_connection():
        log("ERROR: Cannot connect to Windows VM via SSH")
        log("Make sure the VM is running and SSH is configured")
        return 1
    log("  SSH connection OK")
    print()

    # Step 1: Check if server is running
    log("Checking if server is running...")
    if is_server_running():
        log("  Server is running")

        # Step 2: Shutdown
        if not shutdown_server():
            log("ERROR: Failed to send shutdown signal")
            return 1

        # Step 3: Wait for shutdown
        if not wait_for_shutdown():
            log("WARNING: Server may not have shut down cleanly")
    else:
        log("  Server is not running")
    print()

    # Step 3.5: Build if requested (must be after shutdown so DLL is unlocked)
    if args.rebuild:
        if not build_server():
            log("ERROR: Build failed, not starting server")
            return 1
        print()

    # Step 4: Start server
    if not start_server_background():
        log("ERROR: Failed to start server")
        return 1
    print()

    # Step 5: Wait for startup
    if not wait_for_startup():
        log("ERROR: Server failed to start")
        show_recent_log()
        return 1
    print()

    # Brief pause for initialization
    time.sleep(2)

    # Step 6: Health check (local)
    if not health_check():
        log("ERROR: Health check failed")
        show_recent_log()
        return 1
    print()

    # Step 7: Test auction call
    if not test_auction_call():
        log("WARNING: Test auction call failed (server may still be initializing)")
        show_recent_log()
        # Don't fail on this - server is running
    print()

    # Step 8: Check/start Cloudflare tunnel
    log("Checking Cloudflare tunnel...")
    if is_cloudflared_running():
        log("  Cloudflare tunnel is running")
    else:
        log("  Cloudflare tunnel is not running")
        if not start_cloudflared():
            log("WARNING: Could not start Cloudflare tunnel")
            log("  Public endpoint may not be accessible")
    print()

    # Step 9: Verify public endpoint
    if not check_public_endpoint():
        log("WARNING: Public endpoint check failed")
        log("  The tunnel may need time to connect, or there may be a DNS issue")
        # Don't fail - local server is working
    print()

    # Show recent log
    show_recent_log()
    print()

    print("=" * 60)
    log("Server restart completed successfully!")
    print(f"  Local:  http://{VM_IP}:{SERVER_PORT}")
    print(f"  Public: {PUBLIC_URL}")
    print("=" * 60)
    return 0


if __name__ == "__main__":
    sys.exit(main())
