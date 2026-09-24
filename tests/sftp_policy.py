#!/usr/bin/env python3
import importlib.util,pathlib
path=pathlib.Path(__file__).resolve().parents[1]/'deploy/harden-sftp.py'
spec=importlib.util.spec_from_file_location('hardener',path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
source='PasswordAuthentication yes\n\nMatch User upload\n    ChrootDirectory /srv/sftp/test\n    ForceCommand internal-sftp -d /website\n    AllowStreamLocalForwarding yes\n    DisableForwarding no\nMatch all\n\nMatch User other\n    ForceCommand internal-sftp\n    ChrootDirectory /unrelated\n    DisableForwarding no\n'
result,count=module.harden(source)
assert count==1
assert result.startswith('PasswordAuthentication yes\n\nMatch User upload\n    DisableForwarding yes\n    AllowStreamLocalForwarding no\n')
assert 'AllowStreamLocalForwarding yes' not in result
assert result.endswith('    ChrootDirectory /unrelated\n    DisableForwarding no\n')
assert module.harden(result)[0]==result
assert module.harden('Match User admin\n    ForceCommand /bin/bash\n')[0]=='Match User admin\n    ForceCommand /bin/bash\n'
print('PASS: SFTP profile targeting, all-forwarding restriction, unrelated SSH preservation and idempotence')
