#!/usr/bin/env python3
"""
Start the BBA server on the Windows VM via SSH.
Uses the ssh_runner from Practice-Bidding-Scenarios.
"""
import sys
import os

# Add the PBS build-scripts-mac to path
PBS_BUILD_SCRIPTS = os.path.expanduser(
    "/Volumes/Express2T/Development/GitHub/Practice-Bidding-Scenarios/build-scripts-mac"
)
sys.path.insert(0, PBS_BUILD_SCRIPTS)

from ssh_runner import run_windows_command, test_ssh_connection, mac_to_windows_path

# BBA-Tools project root (parent of bba-server)
BBA_CLI_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BBA_SERVER_PATH = os.path.join(BBA_CLI_ROOT, "bba-server")


def start_server(port: int = 5000, background: bool = False):
    """
    Start the BBA server on Windows.

    Args:
        port: Port to run the server on
        background: If True, run in background and return immediately
    """
    print("Starting BBA Server on Windows VM...")

    # Test SSH connection first
    if not test_ssh_connection():
        print("ERROR: Cannot connect to Windows VM via SSH")
        print("Make sure the VM is running and SSH is configured")
        return False

    # Convert path to Windows format
    # BBA-Tools is at G:\BBA-Tools on Windows (sibling of Practice-Bidding-Scenarios)
    windows_server_path = "G:\\BBA-Tools\\bba-server"

    # Build and run command
    if background:
        # Start in background using 'start' command
        cmd = f'cd /d {windows_server_path} && start /b dotnet run --urls=http://0.0.0.0:{port}'
    else:
        # Run in foreground (will block)
        cmd = f'cd /d {windows_server_path} && dotnet run --urls=http://0.0.0.0:{port}'

    print(f"  Server path: {windows_server_path}")
    print(f"  Port: {port}")
    print(f"  Background: {background}")
    print()

    try:
        returncode, stdout, stderr = run_windows_command(
            cmd,
            check=False,
            timeout=None if not background else 30,  # No timeout for foreground
            verbose=True,
        )

        if returncode == 0:
            print("Server started successfully!")
            if stdout:
                print(stdout)
        else:
            print(f"Server exited with code {returncode}")
            if stderr:
                print(f"Error: {stderr}")

        return returncode == 0

    except KeyboardInterrupt:
        print("\nServer stopped by user")
        return True
    except Exception as e:
        print(f"Error starting server: {e}")
        return False


def check_server_status(port: int = 5000):
    """Check if the BBA server is running."""
    print(f"Checking BBA Server status on port {port}...")

    if not test_ssh_connection():
        print("ERROR: Cannot connect to Windows VM via SSH")
        return False

    # Check if dotnet is listening on the port
    cmd = f'netstat -ano | findstr ":{port}"'

    try:
        returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)

        if returncode == 0 and stdout.strip():
            print(f"Server appears to be running:")
            print(stdout)
            return True
        else:
            print("Server is not running")
            return False

    except Exception as e:
        print(f"Error checking status: {e}")
        return False


def stop_server():
    """Stop the BBA server on Windows."""
    print("Stopping BBA Server on Windows VM...")

    if not test_ssh_connection():
        print("ERROR: Cannot connect to Windows VM via SSH")
        return False

    # Find and kill bba-server.exe (or dotnet.exe if running via dotnet run)
    cmd = 'taskkill /f /im bba-server.exe 2>nul || taskkill /f /im dotnet.exe 2>nul || echo "No server process found"'

    try:
        returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=True)
        print(stdout if stdout else "Done")
        return True
    except Exception as e:
        print(f"Error stopping server: {e}")
        return False


def build_server():
    """Build the BBA server on Windows."""
    print("Building BBA Server on Windows VM...")

    if not test_ssh_connection():
        print("ERROR: Cannot connect to Windows VM via SSH")
        return False

    windows_server_path = "G:\\BBA-Tools\\bba-server"
    cmd = f'cd /d {windows_server_path} && dotnet build'

    try:
        returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=True)

        if returncode == 0:
            print("Build successful!")
            if stdout:
                # Print last few lines
                lines = stdout.strip().split('\n')[-10:]
                for line in lines:
                    print(f"  {line}")
        else:
            print(f"Build failed with code {returncode}")
            if stderr:
                print(stderr)
            if stdout:
                print(stdout)

        return returncode == 0

    except Exception as e:
        print(f"Error building server: {e}")
        return False


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Manage BBA Server on Windows VM")
    parser.add_argument("action", choices=["start", "stop", "status", "build"],
                       help="Action to perform")
    parser.add_argument("--port", type=int, default=5000,
                       help="Port to run server on (default: 5000)")
    parser.add_argument("--foreground", "-f", action="store_true",
                       help="Run server in foreground (default is background)")

    args = parser.parse_args()

    if args.action == "start":
        success = start_server(port=args.port, background=not args.foreground)
    elif args.action == "stop":
        success = stop_server()
    elif args.action == "status":
        success = check_server_status(port=args.port)
    elif args.action == "build":
        success = build_server()

    sys.exit(0 if success else 1)
