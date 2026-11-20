# Oink para Linux (fork não oficial)

> **Créditos.** Este projeto é um fork do **[Oink — Oink Is Not only a KVM](https://github.com/oinkvm/oink)**,
> criado por **hachiwii** e pelos contribuidores do repositório original. Todo o núcleo (protocolo
> peer-to-peer, criptografia, pareamento, aplicativo para Windows e macOS) é trabalho deles. Este fork
> só acrescenta o **receptor Linux (X11)**. Se o Oink te ajuda, considere
> [apoiar o autor original](https://ko-fi.com/hachiwii).
>
> Este é um fork modificado e **não é afiliado** ao projeto original. O nome "Oink" pertence a ele
> (veja [LICENSE-ADDITIONAL-TERMS.md](LICENSE-ADDITIONAL-TERMS.md)). O README original está em
> [oinkvm/oink](https://github.com/oinkvm/oink#readme).

## O que é

O Oink é um KVM por software: o teclado e o mouse de um computador controlam outros computadores da
mesma rede, sem cabo e sem servidor, com tráfego criptografado. O original funciona em Windows e macOS.
Este fork faz o **Linux** participar, **como máquina controlada**:

| Recurso | Windows / macOS | Linux (este fork) |
|---|---|---|
| Teclado e mouse controlando outro computador | sim | **não** (só recebe) |
| Ser controlado pelo teclado e mouse de outro | sim | **sim** |
| Área de transferência sincronizada | texto, imagem, arquivos | **só texto simples**, nos dois sentidos |
| Arrastar e soltar, áudio compartilhado | sim | não |
| Interface gráfica | sim | não (linha de comando) |
| Sessão gráfica | qualquer | **X11** (Wayland não funciona) |

Ou seja: o caso de uso é **usar o teclado e o mouse do PC para controlar o notebook Linux** e copiar e
colar texto entre os dois.

## Como funciona

Os computadores se descobrem na rede local e se **pareiam uma vez** com um código de 6 dígitos
mostrado em uma tela e digitado na outra. Depois disso se reconhecem sozinhos.

- **Windows e macOS** rodam o aplicativo oficial (janela com a lista de dispositivos próximos).
  Ele captura o teclado e o mouse e envia aos outros.
- **Linux** roda o `oink-agent`, um serviço em segundo plano sem janela. Ele recebe os eventos e os
  injeta na sessão gráfica pela extensão XTEST do X11, converte o teclado entre os mapeamentos, e
  espelha a área de transferência de texto (X11 `CLIPBOARD`) com o outro lado.
- O protocolo é o mesmo do original (versão 0.3.7), por isso o app oficial conversa com o receptor
  Linux sem nenhuma alteração.

## Instalação e uso

### 1. Linux (o computador controlado)

Requisitos: Debian/Ubuntu ou derivado, 64 bits (x86_64), sessão **Xorg** (na tela de login escolha
"Ubuntu/GNOME on Xorg" ou equivalente), `systemd`, `python3`, `libasound2` e `libssl3`.

```sh
curl -fsSL https://raw.githubusercontent.com/sidneyjfg/oink/linux-port/installers/linux/install.sh | bash
```

O script baixa o binário da [Release](https://github.com/sidneyjfg/oink/releases) deste repositório,
confere o SHA-256 e instala em `~/.local/bin` **sem root e sem Rust**. Ele cria o serviço
`oink-agent` do systemd de usuário, que sobe no login e acha sozinho o `DISPLAY`.

```sh
systemctl --user status oink-agent      # ver se está rodando
journalctl --user -u oink-agent -f      # logs
~/.local/bin/oink-agent status          # estado do agente
./installers/linux/install.sh --uninstall   # remover (mantém o pareamento)
```

Sem Release disponível, use um binário que você compilou: `./install.sh --binary caminho/oink-agent`.

### 2. Windows (o computador que controla)

Execute em um PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File installers\windows\install.ps1
```

O script baixa o **instalador oficial** (MSI) do projeto original, confere o SHA-256 e instala. Não há
versão própria para Windows porque o código dele não foi alterado.

> O MSI oficial não é assinado. Se o **Controle de Aplicativos Inteligente** (Smart App Control) estiver
> ligado, o Windows bloqueia a instalação ("a DLL necessária não pôde ser executada"). Só dá para
> desligá-lo e, em geral, sem volta; é uma decisão sua, o script apenas avisa. Em outros casos, o
> SmartScreen pede *Mais informações → Executar assim mesmo*.

### 3. macOS (o computador que controla)

Instale o aplicativo oficial pelas [Releases do Oink](https://github.com/oinkvm/oink/releases)
(macOS 14.2 ou mais novo). Não precisa de nada deste fork: ele se comporta como o Windows.

### 4. Parear e usar

1. Abra o Oink no PC (Windows/macOS). O notebook aparece em **Dispositivos próximos**; clique nele.
2. O PC mostra um **código de 6 dígitos**.
3. No notebook, rode: `oink-pair 123456` (com o código que apareceu).
4. Pronto. Mova o mouse para a borda da tela do PC configurada para o notebook e o controle passa para
   ele; teclado, rolagem e cliques seguem junto. O que você copiar (texto) em um lado pode ser colado
   no outro.

Se `oink-pair` não for encontrado, o `~/.local/bin` não está no `PATH`: use o caminho completo.

## Limitações conhecidas

- Área de transferência: somente texto simples (sem imagem, HTML ou arquivos; textos enormes
  transferidos em partes, protocolo INCR, são recusados).
- Sem Wayland, sem arrastar e soltar, sem áudio compartilhado, sem interface gráfica no Linux.
- O notebook nunca controla o PC: não há captura de entrada no Linux.
- Testado em Debian 12 com GNOME/Xorg e PC com Windows 11. Outras combinações podem funcionar, mas
  não foram verificadas.

## Compilar do código-fonte

```sh
sudo apt install build-essential pkg-config cmake libssl-dev libasound2-dev libopus-dev \
                 libx11-dev libxtst-dev libxrandr-dev
cargo build --release -p oink-agent        # binário em target/release/oink-agent
```

O binário de release é gerado pelo workflow
[`Linux release`](.github/workflows/linux-release.yml), que roda ao publicar uma tag `v*-linux*`
(por exemplo `git tag v0.3.7-linux2 && git push meu v0.3.7-linux2`) e cria a Release com o arquivo
e o `.sha256`.

## Licença

[GPL-3.0-only](LICENSE) com os [termos adicionais](LICENSE-ADDITIONAL-TERMS.md) do projeto original.
Quem distribui este software ou uma versão modificada deve manter o código-fonte aberto sob a mesma
licença, preservar os créditos e avisos originais e marcar as alterações, e não pode usar o nome
"Oink" para promover produtos derivados. Veja também [NOTICE](NOTICE).
