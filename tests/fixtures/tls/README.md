# Local TLS tunnel test fixtures

These public test-only materials exercise HTTPS certificate validation through
an HTTP CONNECT proxy. `ca.der` is the root certificate trusted only by the
success test; `localhost.der` and `localhost-key.der` are the TLS server chain
and PKCS#8 private key. The server certificate SAN is `DNS:localhost`. Both
certificates are valid from 2026-11-06 16:14:34 UTC through 2036-11-03 16:14:34
UTC. The private key is intentionally checked in as a public test fixture and
must never be used outside tests.

They were generated with OpenSSL 3 using a 2048-bit RSA key and a ten-year
validity. The CA config used `basicConstraints=critical,CA:TRUE`,
`keyUsage=critical,keyCertSign,cRLSign`, and `subjectKeyIdentifier=hash`. The
leaf config used `basicConstraints=critical,CA:FALSE`,
`keyUsage=critical,digitalSignature,keyEncipherment`,
`extendedKeyUsage=serverAuth`, and `subjectAltName=DNS:localhost`.

To rotate, regenerate a test CA and leaf with the same constraints and SAN,
replace all three DER files, then update this README with the actual validity
dates. Keep the CA private key outside the repository and do not add runtime
certificate generation. The original generation commands were:

```sh
cat > ca.cnf <<'EOF'
[req]
distinguished_name=dn
x509_extensions=v3_ca
prompt=no
[dn]
CN=qubit-http test TLS CA
[v3_ca]
basicConstraints=critical,CA:TRUE
keyUsage=critical,keyCertSign,cRLSign
subjectKeyIdentifier=hash
EOF
cat > localhost.cnf <<'EOF'
[req]
distinguished_name=dn
req_extensions=v3_req
prompt=no
[dn]
CN=localhost
[v3_req]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=@alt_names
[alt_names]
DNS.1=localhost
EOF
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out ca.key
openssl req -x509 -new -key ca.key -sha256 -days 3650 -config ca.cnf -out ca.pem
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out localhost.key
openssl req -new -key localhost.key -config localhost.cnf -out localhost.csr
openssl x509 -req -in localhost.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
  -days 3650 -sha256 -extfile localhost.cnf -extensions v3_req -out localhost.pem
openssl x509 -in ca.pem -outform DER -out ca.der
openssl x509 -in localhost.pem -outform DER -out localhost.der
openssl pkcs8 -topk8 -nocrypt -in localhost.key -outform DER -out localhost-key.der
```

The tests consume only these DER files. They do not invoke OpenSSL or disable
certificate or hostname validation.
