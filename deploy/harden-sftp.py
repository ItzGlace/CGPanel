#!/usr/bin/env python3
"""Close all SSH forwarding paths in existing CGPanel SFTP-only profiles."""
import pathlib, re, shutil, subprocess, time


def harden(text):
    blocks=re.split(r'(?im)(?=^[ \t]*Match[ \t]+)',text)
    count=0
    for index,block in enumerate(blocks):
        if not re.search(r'(?im)^\s*ForceCommand\s+internal-sftp(?:\s|$)',block):continue
        if not re.search(r'(?im)^\s*ChrootDirectory\s+/srv/sftp(?:/|\s|$)',block):continue
        lines=[line for line in block.splitlines() if not re.match(r'(?i)^\s*(DisableForwarding|AllowStreamLocalForwarding)\s+',line)]
        # Put the restrictions first in this Match block: sshd uses first values.
        lines[1:1]=['    DisableForwarding yes','    AllowStreamLocalForwarding no']
        blocks[index]='\n'.join(lines)+'\n';count+=1
    return ''.join(blocks),count


def main():
    paths=set(pathlib.Path('/etc/ssh/sshd_config.d').glob('*cgpanel*sftp*.conf'))
    paths.add(pathlib.Path('/etc/ssh/sshd_config.d/80-lunarblush-sftp.conf'))
    changed={}
    try:
        for path in sorted(paths):
            if not path.is_file() or path.is_symlink():continue
            before=path.read_text();after,count=harden(before)
            if count and after!=before:
                backup=path.with_name(path.name+'.pre-cgpanel-'+time.strftime('%Y%m%d-%H%M%S'))
                shutil.copy2(path,backup);changed[path]=before;path.write_text(after)
        subprocess.run(['sshd','-t'],check=True)
        if changed:subprocess.run(['systemctl','reload','ssh'],check=True)
    except Exception:
        for path,before in changed.items():path.write_text(before)
        subprocess.run(['sshd','-t'],check=True)
        if changed:subprocess.run(['systemctl','reload','ssh'],check=True)
        raise
    print(f'SFTP forwarding restrictions checked; {len(changed)} profile file(s) updated')

if __name__=='__main__':main()
