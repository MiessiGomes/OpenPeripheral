<p align="center">
  <img src="icons/gemini-banner.svg" alt="OpenPeripheral" width="800" />
</p>

# OpenPeripheral

Controle de periféricos no Linux a partir de engenharia reversa do software e do hardware oficiais. A ideia é um painel único, open source, onde cada dispositivo entra com o que o firmware realmente expõe — sem colar código GPL e sem exploits.

A UI abre **numa janela própria** (WebKit embutido, como o Discord). O HTTP fica só em `127.0.0.1:7840` para servir o painel. O pacote `.deb` instala o binário, o atalho e as **regras udev** (reload + trigger no `postinst`). Depois da instalação não é preciso copiar `99-hyperx-duocast.rules` à mão.

## Índice

- [Dispositivos portados](#dispositivos-portados)
- [Fila (ainda sem porto)](#fila-ainda-sem-porto)
- [Instalar](#instalar)
- [Desinstalar](#desinstalar)
- [Build a partir do código](#build-a-partir-do-código)
- [Gerar o `.deb`](#gerar-o-deb)
- [udev a partir do source (sem `.deb`)](#udev-a-partir-do-source-sem-deb)
- [Como contribuir um periférico novo](#como-contribuir-um-periférico-novo)
- [Licença](#licença)

Agentes: ver [AGENTS.md](AGENTS.md) (pastas por periférico).

## Dispositivos portados

| Periférico | Tipo | Estado |
|---|---|---|
| HyperX DuoCast | microfone | suportado (LED, polar, mute, ganho/high-pass no teste, mixers ALSA) |
| AJAZZ AJ179 | mouse | suportado (DPI+cores, taxa, LED, LOD, bateria, botões; cabo `248a:5c2e` e dongle `249a:5c2f`) |

## Fila (ainda sem porto)

| Periférico | Tipo | Notas |
|---|---|---|
| Corsair Void Elite Wireless | headset | pendente |
| Blitzwolf BW-KB1 | teclado | pendente |

PRs que adicionem um destes (ou outro) são bem-vindos. Prefira evidência do artefacto (HID, USB, dumps) a adivinhar o protocolo.

## Instalar

1. Baixe o `.deb` da [release no GitHub](https://github.com/MiessiGomes/OpenPeripheral/releases) ou [gere localmente](#gerar-o-deb).
2. Instale **na raiz do repositório** (não em `packaging/`). O `apt` pede a senha **nessa vez** e o `postinst` aplica o udev:

```bash
cd /caminho/para/OpenPeripheral
sudo apt install ./target/debian/openperipheral_0.2.0-1_amd64.deb
```

A partir de uma release, o ficheiro pode estar noutro sítio; use o caminho real, sem o glob `*.deb` se o `apt` não expandir o `*`.

3. Abra **OpenPeripheral** no menu de aplicações ou rode `openperipheral`. Se o microfone já estava ligado, reconecte o USB uma vez para o hidraw herdar as permissões novas.

O CLI fica em `duocast` (`duocast list`, `duocast led`, `duocast hid`, `duocast aj179 show`, …).

Atualizações: a UI consulta a API pública do GitHub (`releases/latest`, senão a tag mais recente) e avisa quando a versão instalada é mais antiga. Não usa Dependabot.

## Desinstalar

Na raiz de qualquer sítio (o pacote já está no sistema):

```bash
sudo apt remove openperipheral
```

Para apagar também ficheiros de configuração que o `dpkg` marque como conffiles:

```bash
sudo apt purge openperipheral
```

Isto remove o binário, o atalho, o ícone e as regras udev do pacote. O `postrm` recarrega o udev. Preferências em `~/.config/openperipheral/` ficam no home; apague essa pasta à mão se quiser um reset completo:

```bash
rm -rf ~/.config/openperipheral
```

## Build a partir do código

No ambiente Rust que você usa:

```bash
cargo test
cargo build --release --bin duocast-ui
./run
```

Dependências de sistema: `pkg-config`, `libusb-1.0`, `libudev`, `libgtk-3`, `libwebkit2gtk-4.1`, `libayatana-appindicator3`, `alsa-utils` (`arecord`/`aplay`/`amixer`), `xdotool`, `wmctrl`. Minimizar envia a janela para a bandeja; o X (ou Sair na bandeja) termina Chrome e o servidor HTTP.

### Gerar o `.deb`

```bash
cargo install cargo-deb
./packaging/build-deb.sh
# imprime o caminho, por exemplo:
#   target/debian/openperipheral_0.2.0-1_amd64.deb
sudo apt install ./target/debian/openperipheral_0.2.0-1_amd64.deb
```

O pacote instala:

- `/usr/bin/openperipheral` — dashboard
- `/usr/bin/duocast` — CLI
- `/usr/lib/udev/rules.d/99-hyperx-duocast.rules`
- `/usr/lib/udev/rules.d/99-ajazz-aj179.rules`
- `/usr/share/applications/openperipheral.desktop`

O `postinst` corre `udevadm control --reload-rules` e `udevadm trigger`.

### udev a partir do source (sem `.deb`)

Só neste caso copie as regras uma vez:

```bash
sudo cp src/peripherals/hyperx_duocast/udev/99-hyperx-duocast.rules \
        src/peripherals/ajazz/udev/99-ajazz-aj179.rules \
        /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

## Como contribuir um periférico novo

1. Documente IDs USB (`lsusb -v`), interfaces HID e o que o software original grava (feature reports, não exploits).
2. Pasta nova em `src/peripherals/<família>/` (ver `AGENTS.md`), protocolo com testes de frames, card no dashboard (`pages`).
3. Regras udev na mesma pasta (`…/udev/`) e `assets` no `Cargo.toml`.
4. `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

O DuoCast é o modelo de referência: dois USB (`03F0:098C` LED, `03F0:0A8C` áudio HID `0x66`). Não envie o opcode `0x02` nesse HID (reennumera o dispositivo).

O AJ179 usa HID vendor de 32 bytes na interface 2 (`HidD`/`hid_write` no Driver (X), checksum = soma dos bytes 4–30). Não envie os comandos de OTA do flasher.

## Licença

MIT. Ver [LICENSE](LICENSE).
