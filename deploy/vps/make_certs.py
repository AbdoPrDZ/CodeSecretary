"""Create a private CA and a server certificate for an IP address.

    python deploy/vps/make_certs.py <ip> <out dir>

Writes `ca.crt` / `ca.key` (10 years) once and reuses them, then a fresh
`server.crt` / `server.key` (825 days) with the IP as subject alternative
name. Clients (the agent, browsers) must trust `ca.crt`. Keys stay in the
git-ignored output directory. Requires `cryptography`.
"""

from __future__ import annotations

import ipaddress
import sys
from datetime import UTC, datetime, timedelta
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID


def _write_key(path: Path, key) -> None:
  path.write_bytes(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                     serialization.NoEncryption()))


def _load_or_create_ca(out: Path):
  crt, key = out / 'ca.crt', out / 'ca.key'
  if crt.exists() and key.exists():
    return (x509.load_pem_x509_certificate(crt.read_bytes()),
            serialization.load_pem_private_key(key.read_bytes(), password=None))
  ca_key = ec.generate_private_key(ec.SECP256R1())
  name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'CodeSecretary private CA')])
  now = datetime.now(UTC)
  ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
        .public_key(ca_key.public_key()).serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(minutes=5)).not_valid_after(now + timedelta(days=3650))
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .add_extension(x509.KeyUsage(digital_signature=True, key_cert_sign=True, crl_sign=True,
                                     content_commitment=False, key_encipherment=False,
                                     data_encipherment=False, key_agreement=False,
                                     encipher_only=False, decipher_only=False), critical=True)
        .add_extension(x509.SubjectKeyIdentifier.from_public_key(ca_key.public_key()), critical=False)
        .sign(ca_key, hashes.SHA256()))
  crt.write_bytes(ca.public_bytes(serialization.Encoding.PEM))
  _write_key(key, ca_key)
  return ca, ca_key


def main() -> None:
  ip, out = ipaddress.ip_address(sys.argv[1]), Path(sys.argv[2])
  out.mkdir(parents=True, exist_ok=True)
  ca, ca_key = _load_or_create_ca(out)
  key = ec.generate_private_key(ec.SECP256R1())
  now = datetime.now(UTC)
  cert = (x509.CertificateBuilder()
          .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, str(ip))]))
          .issuer_name(ca.subject).public_key(key.public_key())
          .serial_number(x509.random_serial_number())
          .not_valid_before(now - timedelta(minutes=5)).not_valid_after(now + timedelta(days=825))
          .add_extension(x509.SubjectAlternativeName([x509.IPAddress(ip)]), critical=False)
          .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
          .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
          .add_extension(x509.AuthorityKeyIdentifier.from_issuer_public_key(ca_key.public_key()),
                         critical=False)
          .sign(ca_key, hashes.SHA256()))
  (out / 'server.crt').write_bytes(cert.public_bytes(serialization.Encoding.PEM))
  _write_key(out / 'server.key', key)
  print(f'certificate for {ip} written to {out}')


if __name__ == '__main__':
  main()
