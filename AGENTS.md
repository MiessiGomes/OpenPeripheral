# OpenPeripheral — guia para agentes

Painel Linux para periféricos reconstruídos a partir do software/hardware oficiais (HID, USB, udev). Um binário (`openperipheral` / `duocast`), vários dispositivos. MIT. Sem colar GPL, sem exploits, sem OTA/firmware flasher.

## Layout

```
src/
  lib.rs, error.rs, rgb.rs, hid.rs, update.rs   # casca partilhada
  bin/duocast-ui.rs, main.rs                    # UI HTTP+GTK e CLI
  peripherals/
    hyperx_duocast/   # microfone: protocol, device, mixer, monitor, udev
    ajazz/            # AJ179 (e futuros AJ*); udev da família
web/                  # dashboard comum
  peripherals/hyperx_duocast/mic3d.js
```

Nomes de pasta = família (marca). Modelos = ficheiros/módulos dentro (`aj179.rs`).

## Novo periférico

1. Pasta `src/peripherals/<família>/` + `mod.rs`; exportar em `src/peripherals/mod.rs`.
2. Protocolo isolado com testes de frames (dumps do artefacto / HID ao vivo).
3. `udev/*.rules` na mesma pasta; entrada em `Cargo.toml` `[package.metadata.deb] assets`.
4. Card no dashboard (`pages`) e subcomando CLI se fizer sentido.
5. `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`.

Evidência: IDs USB, interface HID, o que o software original grava. Não sondar opcodes desconhecidos.

## Ambiente

Rust que o desenvolvedor já usa. Python (se precisar): o env activo; instalar com `uv pip install`, nunca `pip install` nu.

## Git nesta máquina

Commits/PR como **MiessiGomes**. Não alterar `git config`. Não fazer commit sem pedido.

## Referência rápida

- DuoCast: `03F0:098C` LED (não hidraw na iface 0), `03F0:0A8C` áudio HID `0x66`. Não enviar opcode `0x02` no 0x66.
- AJ179: iface 2, relatório 32 B, checksum = soma `[4..=30]`. Não enviar OTA.
