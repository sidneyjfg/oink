# Instaladores

Este fork adiciona um **receptor Linux (X11)** ao Oink: o teclado e o mouse de um PC
Windows ou macOS controlam o Linux, e o texto copiado é sincronizado nos dois sentidos.
O Linux **só recebe** controle; não captura o próprio teclado e mouse.

Licença: GPL-3.0-only com os termos adicionais do projeto original (veja `LICENSE` e
`LICENSE-ADDITIONAL-TERMS.md`). Este é um fork modificado; o nome "Oink" pertence ao
projeto original.

## Linux (Debian/Ubuntu e derivados, x86_64, sessão X11)

```sh
curl -fsSL https://raw.githubusercontent.com/sidneyjfg/oink/linux-port/installers/linux/install.sh | bash
```

Ou, com o repositório clonado: `./installers/linux/install.sh`.

- Instala em `~/.local` (sem root, sem Rust) e cria o serviço `oink-agent` do systemd de usuário,
  que sobe no login e acha sozinho o `DISPLAY` e o `XAUTHORITY`.
- Precisa de `libasound2` e `libssl3` (já vêm em Debian 12 e Ubuntu 22.04+) e `python3`.
- Remover: `./install.sh --uninstall` (o pareamento fica em `~/.local/share/oink`).
- Wayland não é suportado: faça login em uma sessão "Xorg".

### De onde vem o binário

O instalador baixa `oink-agent-linux-x86_64.tar.gz` da última Release **deste repositório** e confere o
SHA-256. A Release é criada pelo workflow `Linux release`
(Actions → *Linux release* → *Run workflow*, informando uma tag como `v0.3.7-linux1`).
Sem Release ainda, use `./install.sh --binary caminho/do/oink-agent`.

### Parear com o PC

1. No Oink do PC, clique no notebook em *Dispositivos próximos*. O PC mostra um código de 6 dígitos.
2. No notebook: `oink-pair 123456`.

## Windows

```powershell
powershell -ExecutionPolicy Bypass -File installers\windows\install.ps1
```

Baixa o **MSI oficial** do projeto original (o código do Windows não foi alterado, então é compatível),
confere o SHA-256 e instala. O MSI não é assinado: com o *Smart App Control* ligado o Windows o bloqueia
e só dá para desligar o controle, em geral sem volta. O script avisa e nunca altera essa configuração.

## Limitações atuais

Só texto simples na área de transferência (sem imagem, HTML ou arquivos); sem arrastar e soltar,
áudio, nem interface gráfica no Linux (use a linha de comando: `oink-agent status`, `devices`, `pairing`).
