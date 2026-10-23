#!/bin/sh
# Corrige pacote a meio (postinst/postrm com CRLF) e faz purge.
set -e
for s in postinst postrm; do
  f="/var/lib/dpkg/info/openperipheral.$s"
  printf '%s\n' '#!/bin/sh' 'exit 0' >"$f"
  chmod 755 "$f"
done
dpkg --purge openperipheral
echo "Pacote removido. Reinstale a partir da raiz do repositório:"
echo "  sudo apt install ./target/debian/openperipheral_0.1.0-1_amd64.deb"
