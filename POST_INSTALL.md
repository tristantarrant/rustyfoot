# Post OS Install Guide

Steps to configure a Raspberry Pi after a fresh OS install for use as a Rustyfoot audio appliance.

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

Scan for available networks:

```bash
nmcli device wifi list
```

Connect to a network:

```bash
sudo nmcli device wifi connect "SSID" password "password"
```

Connect to a hidden network:

```bash
sudo nmcli device wifi connect "SSID" password "password" hidden yes
```

Manage saved connections:

```bash
nmcli connection show                                        # list saved connections
nmcli connection delete "SSID"                               # remove a connection
nmcli connection modify "SSID" connection.autoconnect yes    # auto-connect on boot
nmcli connection modify "SSID" connection.autoconnect-priority 10  # prefer this network
```

## Disable Unnecessary Services

These services are not needed on a dedicated audio appliance and add significant boot time (especially cloud-init and apparmor):

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
```

## Expected Boot Time

After these optimizations, typical boot time on a Pi 5 with SD card:

| Stage | Time |
|-------|------|
| Kernel | ~3s |
| Userspace | ~5s |
| **Total** | **~8s** |

The LV2 prefetch service (included in rustyfoot-modhost) warms the page cache before mod-host starts, reducing plugin scan time from ~33s to <1s.
