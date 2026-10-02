#!/usr/bin/env python3
import argparse, datetime, os, pathlib, shutil, sqlite3, stat, sys, tarfile, tempfile

def main():
    p = argparse.ArgumentParser(description='Online backup of OpenIndieCommerce state.')
    p.add_argument('--db', default=os.getenv('OIC_DB', '/var/lib/openindiecommerce/openindiecommerce.sqlite3'))
    p.add_argument('--entitlement-secret-file', default=os.getenv('OIC_ENTITLEMENT_KEY_SECRET_FILE', ''))
    p.add_argument('--output-dir', default='/var/backups/openindiecommerce')
    a = p.parse_args()
    db = pathlib.Path(a.db)
    out = pathlib.Path(a.output_dir)
    if not db.is_file():
        sys.exit(f'database not found: {db}')
    secret = pathlib.Path(a.entitlement_secret_file) if a.entitlement_secret_file else None
    if secret is not None and not secret.is_file():
        sys.exit(f'entitlement secret not found: {secret}')
    out.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now(datetime.UTC).strftime('%Y%m%dT%H%M%SZ')
    final = out / f'openindiecommerce-{stamp}.tar.gz'
    with tempfile.TemporaryDirectory(prefix='oic-backup-') as td:
        td = pathlib.Path(td)
        dbcopy = td / 'openindiecommerce.sqlite3'
        src = sqlite3.connect(f'file:{db}?mode=ro', uri=True)
        dst = sqlite3.connect(dbcopy)
        with dst:
            src.backup(dst)
        src.close(); dst.close()
        members = [(dbcopy, 'openindiecommerce.sqlite3')]
        if secret is not None:
            secretcopy = td / 'entitlement-key-secret'
            shutil.copyfile(secret, secretcopy)
            secretcopy.chmod(stat.S_IRUSR)
            members.append((secretcopy, 'entitlement-key-secret'))
        with tarfile.open(final, 'w:gz') as tar:
            for path, arcname in members:
                tar.add(path, arcname=arcname)
    final.chmod(stat.S_IRUSR | stat.S_IWUSR)
    print(final)

if __name__ == '__main__':
    main()
