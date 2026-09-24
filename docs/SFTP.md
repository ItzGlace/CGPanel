# SFTP website uploads

Lunarblush uses its separately provisioned SFTP account on SSH port 22. In an SFTP client, select **SFTP**, the server IP, the upload username and its upload password. The initial directory is `/website`; public website files belong under `/website/public`. These credentials are separate from the panel login. CGPanel does not automatically turn every panel account into an SSH/SFTP account.

SFTP-only accounts are confined to a root-owned chroot and use `internal-sftp`. Shell commands, PTYs, TCP forwarding, Unix-socket forwarding, agent forwarding, X11 and tunnels must remain disabled. Workspace ownership and shared group permissions control which files can be edited. Upload temporary files and rename them into place when replacing live content.

Starting with v0.4.1, installation and updates run `deploy/harden-sftp.py`. It adds `DisableForwarding yes` and `AllowStreamLocalForwarding no` inside existing CGPanel SFTP-only Match blocks whose chroot is below `/srv/sftp`. It targets `*cgpanel*sftp*.conf` and the existing `80-lunarblush-sftp.conf` under `/etc/ssh/sshd_config.d`. It preserves passwords, account names, mounts and unrelated SSH access. Changed files are backed up alongside the configuration, syntax is validated before reload, and failed changes are restored. Custom SSH profiles outside these paths require administrator review.

Administrators can verify the effective configuration with:

```sh
sshd -t
sshd -T -C user=UPLOAD_USER,host=SERVER_HOST,addr=CLIENT_IP | \
  grep -E 'chrootdirectory|forcecommand|disableforwarding|allowstreamlocalforwarding'
```

Expect the intended chroot, `internal-sftp`, `disableforwarding yes` and `allowstreamlocalforwarding no`. Also check every chroot parent is root-owned and not writable by the upload user, the workspace bind mount is mounted, and SSH port 22 is reachable.

`tests/sftp_inbound.py` runs real cross-host checks with Python Paramiko: host-key pinning, a 2 MiB binary upload/download with SHA-256 comparison, rename, permissions, deletion, chroot traversal/symlink denial, shell denial and TCP-forwarding rejection. Feed a JSON object on standard input with `host`, `username`, `password`, `directory` optional `port` (defaults to 22), and `host_key` (verified `ssh-ed25519 AAAA...` public key). Obtain host keys over a trusted administrator connection. Never put passwords in command-line arguments, source control or test logs. The test creates and removes a uniquely named directory under the chosen upload directory.
