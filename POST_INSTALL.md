# Post OS Install Guide

Steps to configure a Raspberry Pi after a fresh Raspberry Pi OS install for use as a Rustyfoot audio appliance.

## OS Installation

Use the **Raspberry Pi Imager** with these settings:

- **OS**: Raspberry Pi OS Lite (64-bit) — no desktop environment needed
- **Pre-configuration** (gear icon):
  - Hostname: `tatooine`
  - Enable SSH with your public key
  - Username: `pi`
  - Set locale/timezone
  - **Do not configure WiFi** — avoids wpa_supplicant/NM delays at boot; add later if needed

## Prerequisites

Install the rustyfoot packages:

```bash
sudo dpkg -i rustyfoot-modhost_*.deb rustyfoot_*.deb
sudo apt-get install -f  # resolve any missing dependencies
```

Deploy rustyfoot-hmi via `flutterpi_tool`.

## Network Configuration

Disable IPv6 on the wired connection (eliminates router advertisement delays):

```bash
sudo nmcli connection modify 'Wired connection 1' ipv6.method disabled
```

Disable DHCP Address Conflict Detection (saves ~2s on lease acquisition):

```bash
sudo nmcli connection modify 'Wired connection 1' ipv4.dad-timeout 0
```

## WiFi

WiFi is not needed for normal operation and significantly impacts boot time. If you do need it, configure it to not block boot:

```bash
# Connect to a network
sudo nmcli device wifi connect "SSID" password "password"

# Disable auto-connect so it doesn't slow boot
sudo nmcli connection modify "SSID" connection.autoconnect no
```

To connect manually when needed:

```bash
nmcli connection up "SSID"
```

If WiFi is not needed at all, mask wpa_supplicant to prevent NM from waiting for it (~3 minute timeout on boot):

```bash
sudo systemctl disable --now wpa_supplicant.service
sudo systemctl mask wpa_supplicant.service
```

To re-enable WiFi later:

```bash
sudo systemctl unmask wpa_supplicant.service
sudo systemctl enable --now wpa_supplicant.service
```

Manage saved connections:

```bash
nmcli connection show                                        # list saved connections
nmcli connection delete "SSID"                               # remove a connection
nmcli connection modify "SSID" connection.autoconnect yes    # auto-connect on boot
nmcli connection modify "SSID" connection.autoconnect-priority 10  # prefer this network
```

## Disable Unnecessary Services

These services are not needed on a dedicated audio appliance and add boot time:

```bash
# Cloud provisioning (biggest boot time impact ~25s)
sudo systemctl mask cloud-init-local.service cloud-config.service \
    cloud-final.service cloud-init-network.service cloud-init.target

# Security profiles (not needed on single-purpose device)
sudo systemctl disable apparmor.service

# Disk management daemon
sudo systemctl disable udisks2.service

# Network services (nothing depends on network-online.target)
sudo systemctl disable NetworkManager-wait-online.service \
    NetworkManager-dispatcher.service

# Filesystem scrubbing (not useful on SD card)
sudo systemctl disable e2scrub_all.timer e2scrub_reap.service

# Console keyboard setup (not needed without physical keyboard)
sudo systemctl disable console-setup.service keyboard-setup.service

# One-time SSH setup (already done after first boot)
sudo systemctl disable regenerate_ssh_host_keys.service \
    sshd-keygen.service sshswitch.service

# Kernel crash dump storage
sudo systemctl disable systemd-pstore.service

# Automatic apt updates (causes I/O storms on SD card)
sudo systemctl disable apt-daily.timer apt-daily-upgrade.timer

# Man page index rebuilds
sudo systemctl disable man-db.timer

# EEPROM update check (33s on boot for a no-op check)
sudo systemctl disable rpi-eeprom-update.service
```

## Disable Initramfs

The Pi boots directly from an ext4 partition on the SD card — no LVM, encryption, or network root. The initramfs adds ~30s of boot time for decompression and is not needed:

```bash
sudo sed -i 's/^auto_initramfs=1/auto_initramfs=0/' /boot/firmware/config.txt
```

The initramfs files remain on disk. To re-enable if ever needed:

```bash
sudo sed -i 's/^auto_initramfs=0/auto_initramfs=1/' /boot/firmware/config.txt
```

## Service Ordering: Rustyfoot After Base OS

The rustyfoot audio stack (jackd-modhost, rustyfoot, rustyfoot-hmi) should start after the base OS is fully ready. This prevents I/O contention during boot and avoids systemd dependency cycles.

Create a `rustyfoot.target` that starts after `multi-user.target`:

```bash
sudo tee /etc/systemd/system/rustyfoot.target > /dev/null << 'EOF'
[Unit]
Description=Rustyfoot Audio Stack
After=multi-user.target
Wants=rustyfoot.service rustyfoot-hmi.service

[Install]
WantedBy=multi-user.target
EOF
```

Move rustyfoot services out of multi-user.target and into rustyfoot.target:

```bash
sudo systemctl disable rustyfoot.service rustyfoot-hmi.service
sudo systemctl enable rustyfoot.target
```

Add ordering to rustyfoot.service so it waits for multi-user.target:

```bash
sudo mkdir -p /etc/systemd/system/rustyfoot.service.d
sudo tee /etc/systemd/system/rustyfoot.service.d/after-boot.conf > /dev/null << 'EOF'
[Unit]
After=multi-user.target
EOF
```

## HMI Exit to Shell

The HMI's power menu includes an "Exit to Shell" option that stops the HMI service, releasing the display and input devices so you can access the console. This requires a sudoers rule (included in rustyfoot-modhost), but verify it's present:

```bash
cat /etc/sudoers.d/modhost-power
# Should contain:
# modhost ALL=(ALL) NOPASSWD: /usr/sbin/shutdown, /usr/sbin/reboot, /usr/bin/systemctl stop rustyfoot-hmi.service
```

Note: FlutterPi takes exclusive control of DRM/KMS and input devices, so Ctrl+Alt+F2 (VT switching) does not work while the HMI is running. The "Exit to Shell" menu option is the only way to get a local console without SSH.

## LV2 Prefetch (Not Recommended)

Previous versions included an `lv2-prefetch.service` to warm the page cache before mod-host starts. This is **no longer recommended** because:

- At `idle` I/O priority: gets starved by other boot services, takes minutes to complete, and blocks multi-user.target
- At `best-effort` I/O priority: starves other boot services (tmpfiles-setup, NM), making overall boot much slower
- The benefit (faster plugin scan) is marginal on Pi 5 with a good SD card

If `lv2-prefetch.service` exists, disable it:

```bash
sudo systemctl disable lv2-prefetch.service
sudo rm -f /etc/systemd/system/lv2-prefetch.service
```

## Expected Boot Time

After these optimizations, typical boot time on a Pi 5 with SD card (Samsung EVO Select 128GB):

| Stage | Time |
|-------|------|
| Kernel | ~3s |
| Userspace to multi-user.target | ~3s |
| Rustyfoot stack ready | ~6s after multi-user.target |
| **Total to audio ready** | **~12s** |
| SSH accessible | ~6s |

Note: A dirty shutdown (power loss) adds ~25s to kernel time due to firmware-level fsck on the boot partition. Always shut down cleanly via the HMI power menu or `sudo shutdown now`.
