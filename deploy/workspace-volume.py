#!/usr/bin/env python3
"""Root-only bounded workspace volumes. Called with a validated tenant/app and MiB.

An ext4 image gives the application and SFTP the same kernel-enforced disk boundary.
Existing data is retained in a root-only recovery directory during first migration.
"""
import fcntl, json, os, pathlib, re, shutil, subprocess, sys, time

def run(*args):
    return subprocess.run(args,check=True,capture_output=True,text=True,timeout=300).stdout.strip()

def main():
    assert os.geteuid()==0
    tenant,app,size=sys.argv[1:4];size=int(size)
    if not all(re.fullmatch('[a-f0-9]{32}',s) for s in (tenant,app)) or not 128<=size<=1048576:raise ValueError('Invalid volume request')
    base=pathlib.Path('/var/lib/cgpanel-volumes');base.mkdir(mode=0o700,exist_ok=True)
    with (base/'lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        folder=pathlib.Path('/srv/cgpanel/tenants')/tenant/'apps'/app
        if folder.is_symlink() or not folder.is_dir():raise ValueError('Workspace must be a real directory')
        image=base/(app+'.img');record=base/(app+'.json');mount=base/('stage-'+app)
        uid=int(run('id','-u','cg_'+tenant[:20]));gid=int(run('id','-g','cg_'+tenant[:20]))
        old_mode=folder.stat().st_mode&0o7777;old_owner=(folder.stat().st_uid,folder.stat().st_gid)
        if record.exists():
            info=json.loads(record.read_text())
            if info['tenant']!=tenant or info['app']!=app:raise ValueError('Volume ownership mismatch')
            if size<info['size_mb']:raise ValueError('Volumes can grow in place. To shrink, back up and restore into a smaller application.')
            run('mountpoint','-q',str(folder))
            device=run('findmnt','-n','-o','SOURCE','--mountpoint',str(folder))
            backing=run('losetup','-n','-O','BACK-FILE',device)
            if pathlib.Path(backing)!=image:raise ValueError('Unexpected workspace mount source')
            if size>info['size_mb']:
                run('fallocate','-l',str(size*1024*1024),str(image));run('losetup','-c',device);run('resize2fs',device)
                info['size_mb']=size;record.write_text(json.dumps(info));record.chmod(0o600)
            print(json.dumps(info));return
        if image.exists():raise ValueError('Unfinished volume migration exists; inspect the recovery files before retrying')
        if shutil.disk_usage(base).free<(size+256)*1024*1024:raise ValueError('Insufficient server disk space for this allocation')
        # Services must be stopped by the broker before calling this helper.
        # Lock out new file-manager/SFTP opens while root copies the old directory.
        os.chown(folder,0,0);folder.chmod(0o700)
        backup=folder.with_name(app+'.pre-volume-'+str(int(time.time())))
        moved=False;mounted=False;binds=[];fstab=pathlib.Path('/etc/fstab');old_fstab=fstab.read_text()
        try:
            run('fallocate','-l',str(size*1024*1024),str(image));image.chmod(0o600)
            run('mkfs.ext4','-q','-F','-m','0',str(image))
            mount.mkdir(mode=0o700);run('mount','-o','loop,nosuid,nodev',str(image),str(mount));mounted=True
            run('cp','-a','--',str(folder)+'/.',str(mount))
            os.chown(mount,uid,gid);mount.chmod(old_mode|0o2000)
            run('sync','-f',str(mount));run('umount',str(mount));mounted=False
            folder.rename(backup);moved=True;folder.mkdir(mode=0o700)
            run('mount','-o','loop,nosuid,nodev',str(image),str(folder))
            # Bind mounts must follow the workspace volume at boot and after migration.
            lines=[];binds=[]
            for line in old_fstab.splitlines():
                parts=line.split()
                if len(parts)>=4 and parts[0]==str(folder) and parts[1].startswith('/srv/sftp/'):
                    if pathlib.Path(parts[1]).is_symlink():raise ValueError('Unexpected SFTP mount target')
                    binds.append(parts[1]);parts[3]+=',x-systemd.requires-mounts-for='+str(folder);line=' '.join(parts)
                lines.append(line)
            lines.append(f'{image} {folder} ext4 loop,nosuid,nodev,nofail 0 0')
            fstab.write_text('\n'.join(lines)+'\n')
            for target in binds:
                if subprocess.run(['mountpoint','-q',target]).returncode==0:run('umount',target)
                run('mount','--bind',str(folder),target)
            info={'tenant':tenant,'app':app,'size_mb':size,'path':str(folder),'recovery':str(backup),'enforced':True}
            record.write_text(json.dumps(info));record.chmod(0o600);run('systemctl','daemon-reload')
            print(json.dumps(info))
        except Exception:
            if mounted:subprocess.run(['umount',str(mount)])
            if moved:
                subprocess.run(['umount',str(folder)])
                if not os.path.ismount(folder) and not any(folder.iterdir()):folder.rmdir();backup.rename(folder)
            fstab.write_text(old_fstab)
            if folder.exists() and not os.path.ismount(folder):os.chown(folder,*old_owner);folder.chmod(old_mode)
            for target in binds:
                subprocess.run(['umount',target],capture_output=True)
                subprocess.run(['mount','--bind',str(folder),target],check=True)
            if not run('losetup','-j',str(image)) and not record.exists():image.unlink(missing_ok=True)
            raise
        finally:
            if mount.exists() and not os.path.ismount(mount):mount.rmdir()

if __name__=='__main__':main()
