#!/usr/bin/env python3
import argparse, pathlib, sqlite3, tarfile, tempfile

def main():
    p = argparse.ArgumentParser(description='Verify an OpenIndieCommerce backup archive.')
    p.add_argument('archive')
    a = p.parse_args()
    archive = pathlib.Path(a.archive)
    if not archive.is_file():
        raise SystemExit(f'backup not found: {archive}')
    with tempfile.TemporaryDirectory(prefix='oic-verify-') as td:
        td = pathlib.Path(td)
        with tarfile.open(archive, 'r:gz') as tar:
            names = set(tar.getnames())
            if 'openindiecommerce.sqlite3' not in names:
                raise SystemExit('backup missing openindiecommerce.sqlite3')
            for name in names:
                if pathlib.Path(name).name != name:
                    raise SystemExit('unsafe backup member')
                member = tar.getmember(name)
                if not member.isfile():
                    continue
                src = tar.extractfile(member)
                if src is None:
                    raise SystemExit(f'cannot read {name}')
                (td / name).write_bytes(src.read())
        db_path = td / 'openindiecommerce.sqlite3'
        db = sqlite3.connect(f'file:{db_path}?mode=ro', uri=True)
        integrity = db.execute('PRAGMA integrity_check').fetchone()[0]
        tables = {row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        db.close()
        if integrity != 'ok':
            raise SystemExit(f'SQLite integrity_check failed: {integrity}')
        required = {
            'products', 'prices', 'checkout_sessions', 'commerce_orders',
            'commerce_events', 'merchant_webhook_endpoints', 'webhook_deliveries'
        }
        if not required.issubset(tables):
            raise SystemExit(f'database missing tables: {sorted(required - tables)}')
        secret = td / 'entitlement-key-secret'
        if secret.exists() and len(secret.read_bytes().strip()) < 32:
            raise SystemExit('entitlement-key-secret is shorter than 32 bytes')
    print('OIC_BACKUP_VERIFY_OK')

if __name__ == '__main__':
    main()
