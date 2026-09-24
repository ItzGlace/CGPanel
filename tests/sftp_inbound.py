#!/usr/bin/env python3
"""Run on another host. Read credentials + verified host key as JSON on stdin."""
import base64,hashlib,io,json,secrets,sys,paramiko

def verify(settings):
    ssh=paramiko.SSHClient()
    key=paramiko.Ed25519Key(data=base64.b64decode(settings['host_key'].split()[1]))
    port=settings.get('port',22)
    ssh.get_host_keys().add(settings['host'] if port==22 else f"[{settings['host']}]:{port}",'ssh-ed25519',key)
    ssh.connect(settings['host'],port=port,username=settings['username'],password=settings['password'],allow_agent=False,look_for_keys=False,timeout=20,auth_timeout=60)
    sftp=ssh.open_sftp();sftp.get_channel().settimeout(30);directory=settings['directory'].rstrip('/')+'/.cgpanel-sftp-qa-'+secrets.token_hex(8)
    try:
        sftp.mkdir(directory);data=secrets.token_bytes(2*1024*1024)
        sftp.putfo(io.BytesIO(data),directory+'/upload.bin',confirm=True)
        sftp.rename(directory+'/upload.bin',directory+'/renamed.bin')
        received=io.BytesIO();sftp.getfo(directory+'/renamed.bin',received)
        assert hashlib.sha256(received.getvalue()).digest()==hashlib.sha256(data).digest()
        sftp.chmod(directory+'/renamed.bin',0o660)
        assert sftp.stat(directory+'/renamed.bin').st_mode & 0o777 == 0o660
        assert sftp.normalize('/../../')=='/'
        if settings.get('restricted',True):
            assert 'etc' not in sftp.listdir('/')
            sftp.symlink('/etc/shadow',directory+'/escape')
            try:
                with sftp.file(directory+'/escape','r') as f:f.read(1)
                raise AssertionError('Chroot escape')
            except OSError:pass
            finally:sftp.remove(directory+'/escape')
            try:
                channel=ssh.get_transport().open_channel('direct-tcpip',('127.0.0.1',22),('127.0.0.1',0),timeout=10)
            except paramiko.ChannelException as error:assert error.code==1,error
            else:channel.close();raise AssertionError('TCP forwarding allowed')
            _,out,err=ssh.exec_command('id',timeout=10)
            assert 'uid=' not in out.read().decode()+err.read().decode()
        print('PASS: pinned-key SFTP login, 2 MiB upload/download SHA256, rename, chmod, delete'+(', jailed paths, shell and TCP forwarding denial' if settings.get('restricted',True) else ''),flush=True)
    finally:
        for entry in sftp.listdir(directory):sftp.remove(directory+'/'+entry)
        sftp.rmdir(directory);sftp.close();ssh.close()

if __name__=='__main__':verify(json.load(sys.stdin))
