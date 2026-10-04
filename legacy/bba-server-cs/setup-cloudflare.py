#!/usr/bin/env python3
"""
Setup Cloudflare Tunnel on Windows VM via SSH.
Downloads, installs, and configures cloudflared.
"""
import sys
import os
import time

# Add the PBS build-scripts-mac to path
PBS_BUILD_SCRIPTS = os.path.expanduser(
    "/Volumes/Express2T/Development/GitHub/Practice-Bidding-Scenarios/build-scripts-mac"
)
sys.path.insert(0, PBS_BUILD_SCRIPTS)

from ssh_runner import run_windows_command, test_ssh_connection


def log(msg: str):
    """Print timestamped message."""
    from datetime import datetime
    print(f"[{datetime.now().strftime('%H:%M:%S')}] {msg}")


def check_cloudflared_installed() -> bool:
    """Check if cloudflared is already installed."""
    cmd = r'"C:\Program Files (x86)\cloudflared\cloudflared.exe" --version'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)
    if returncode == 0:
        log(f"  cloudflared already installed: {stdout.strip()}")
        return True
    return False


def download_cloudflared() -> bool:
    """Download cloudflared MSI installer."""
    log("Downloading cloudflared...")

    # Use PowerShell to download
    url = "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.msi"
    cmd = f'powershell -Command "Invoke-WebRequest -Uri \'{url}\' -OutFile \'C:\\Temp\\cloudflared.msi\'"'

    # Create temp directory first
    run_windows_command("mkdir C:\\Temp 2>nul", check=False, verbose=False)

    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=True, timeout=120)
    if returncode != 0:
        log(f"  Download failed: {stderr}")
        return False

    log("  Download complete")
    return True


def install_cloudflared() -> bool:
    """Install cloudflared from MSI."""
    log("Installing cloudflared...")

    cmd = 'msiexec /i C:\\Temp\\cloudflared.msi /quiet /norestart'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=True, timeout=120)

    # Give it time to install
    time.sleep(5)

    # Verify installation
    if check_cloudflared_installed():
        log("  Installation successful")
        return True
    else:
        log(f"  Installation may have failed: {stderr}")
        return False


def run_cloudflared(args: str) -> tuple[int, str, str]:
    """Run cloudflared command."""
    cmd = f'"C:\\Program Files (x86)\\cloudflared\\cloudflared.exe" {args}'
    return run_windows_command(cmd, check=False, verbose=True, timeout=60)


def login_cloudflared() -> bool:
    """
    Initiate cloudflared login.
    This will print a URL that the user needs to visit to authorize.
    """
    log("Starting cloudflared login...")
    log("  NOTE: This will print a URL. You need to open it in your browser")
    log("        and select your domain to authorize the tunnel.")
    print()

    # The login command will output a URL
    returncode, stdout, stderr = run_cloudflared("tunnel login")

    print(stdout)
    if stderr:
        print(stderr)

    if returncode == 0:
        log("  Login successful!")
        return True
    else:
        log("  Login may require manual browser authorization")
        log("  Check the URL printed above")
        return False


def create_tunnel(tunnel_name: str = "bba-server") -> str | None:
    """Create a new tunnel and return its ID."""
    log(f"Creating tunnel '{tunnel_name}'...")

    returncode, stdout, stderr = run_cloudflared(f"tunnel create {tunnel_name}")

    print(stdout)
    if returncode != 0:
        # Tunnel might already exist
        if "already exists" in stderr or "already exists" in stdout:
            log("  Tunnel already exists, getting info...")
            returncode, stdout, stderr = run_cloudflared(f"tunnel info {tunnel_name}")
            print(stdout)
        else:
            log(f"  Failed to create tunnel: {stderr}")
            return None

    # Extract tunnel ID from output (it's a UUID)
    import re
    match = re.search(r'([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})', stdout, re.IGNORECASE)
    if match:
        tunnel_id = match.group(1)
        log(f"  Tunnel ID: {tunnel_id}")
        return tunnel_id

    return None


def create_config(tunnel_id: str, hostname: str = "bba.harmonicsystems.com") -> bool:
    """Create cloudflared config file."""
    log("Creating config file...")

    # Get Windows username for path
    returncode, stdout, stderr = run_windows_command("echo %USERNAME%", check=False, verbose=False)
    username = stdout.strip()

    config_dir = f"C:\\Users\\{username}\\.cloudflared"
    config_path = f"{config_dir}\\config.yml"

    # Create config directory
    run_windows_command(f'mkdir "{config_dir}" 2>nul', check=False, verbose=False)

    # Create config content
    config_content = f"""tunnel: {tunnel_id}
credentials-file: {config_dir}\\{tunnel_id}.json

ingress:
  - hostname: {hostname}
    service: http://localhost:5000
  - service: http_status:404
"""

    # Write config file using PowerShell
    # Escape for PowerShell
    escaped_content = config_content.replace('"', '`"').replace('\n', '`n')
    cmd = f'powershell -Command "Set-Content -Path \'{config_path}\' -Value \\"{escaped_content}\\""'

    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=True)

    if returncode != 0:
        # Try alternative method
        log("  Trying alternative config write method...")
        lines = config_content.strip().split('\n')

        # Write first line (overwrite)
        cmd = f'echo tunnel: {tunnel_id} > "{config_path}"'
        run_windows_command(cmd, check=False, verbose=False)

        # Append remaining lines
        cmd = f'echo credentials-file: {config_dir}\\{tunnel_id}.json >> "{config_path}"'
        run_windows_command(cmd, check=False, verbose=False)

        cmd = f'echo. >> "{config_path}"'
        run_windows_command(cmd, check=False, verbose=False)

        cmd = f'echo ingress: >> "{config_path}"'
        run_windows_command(cmd, check=False, verbose=False)

        cmd = f'echo   - hostname: {hostname} >> "{config_path}"'
        run_windows_command(cmd, check=False, verbose=False)

        cmd = f'echo     service: http://localhost:5000 >> "{config_path}"'
        run_windows_command(cmd, check=False, verbose=False)

        cmd = f'echo   - service: http_status:404 >> "{config_path}"'
        run_windows_command(cmd, check=False, verbose=False)

    # Verify config
    cmd = f'type "{config_path}"'
    returncode, stdout, stderr = run_windows_command(cmd, check=False, verbose=False)
    log("  Config file contents:")
    for line in stdout.strip().split('\n'):
        print(f"    {line}")

    return True


def route_dns(tunnel_name: str = "bba-server", hostname: str = "bba.harmonicsystems.com") -> bool:
    """Route DNS to the tunnel."""
    log(f"Routing DNS {hostname} -> tunnel...")

    returncode, stdout, stderr = run_cloudflared(f"tunnel route dns {tunnel_name} {hostname}")

    print(stdout)
    if stderr:
        print(stderr)

    if returncode == 0 or "already exists" in stdout or "already exists" in stderr:
        log("  DNS route configured")
        return True
    else:
        log(f"  DNS routing may have failed")
        return False


def install_service() -> bool:
    """Install cloudflared as a Windows service."""
    log("Installing cloudflared as Windows service...")

    returncode, stdout, stderr = run_cloudflared("service install")

    print(stdout)
    if stderr:
        print(stderr)

    if returncode == 0 or "already exists" in stderr:
        log("  Service installed")
        return True
    else:
        log(f"  Service installation may have failed")
        return False


def start_tunnel(tunnel_name: str = "bba-server") -> bool:
    """Start the tunnel (foreground, for testing)."""
    log(f"Starting tunnel '{tunnel_name}'...")
    log("  Press Ctrl+C to stop")

    returncode, stdout, stderr = run_cloudflared(f"tunnel run {tunnel_name}")

    print(stdout)
    if stderr:
        print(stderr)

    return returncode == 0


def main():
    print("=" * 60)
    print("Cloudflare Tunnel Setup for BBA Server")
    print("=" * 60)
    print()

    # Test SSH connection
    log("Testing SSH connection to Windows VM...")
    if not test_ssh_connection():
        log("ERROR: Cannot connect to Windows VM via SSH")
        return 1
    log("  SSH connection OK")
    print()

    # Check/install cloudflared
    if not check_cloudflared_installed():
        if not download_cloudflared():
            return 1
        if not install_cloudflared():
            return 1
    print()

    # Interactive menu
    while True:
        print()
        print("What would you like to do?")
        print("  1. Login to Cloudflare (required first time)")
        print("  2. Create tunnel 'bba-server'")
        print("  3. Route DNS (bba.harmonicsystems.com)")
        print("  4. Create config file")
        print("  5. Test tunnel (run in foreground)")
        print("  6. Install as Windows service")
        print("  7. Check tunnel status")
        print("  q. Quit")
        print()

        choice = input("Enter choice: ").strip().lower()

        if choice == '1':
            login_cloudflared()
        elif choice == '2':
            tunnel_id = create_tunnel()
            if tunnel_id:
                print(f"\nTunnel ID: {tunnel_id}")
                print("Save this ID - you'll need it for the config file")
        elif choice == '3':
            route_dns()
        elif choice == '4':
            tunnel_id = input("Enter tunnel ID (UUID): ").strip()
            if tunnel_id:
                create_config(tunnel_id)
            else:
                print("No tunnel ID provided")
        elif choice == '5':
            start_tunnel()
        elif choice == '6':
            install_service()
        elif choice == '7':
            run_cloudflared("tunnel list")
        elif choice == 'q':
            break
        else:
            print("Invalid choice")

    print()
    log("Setup complete!")
    return 0


if __name__ == "__main__":
    sys.exit(main())
