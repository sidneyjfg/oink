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

## Comece por aqui

Escolha o que você quer fazer. Cada linha leva ao passo a passo:

| Eu quero... | Vá para |
|---|---|
| Instalar no **Windows** | [Instalar no Windows](#instalar-no-windows) |
| Instalar no **macOS** | [Instalar no macOS](#instalar-no-macos) |
| Instalar no **Linux** | [Instalar no Linux](#instalar-no-linux) |
| Entender como parear e trocar o controle | [Conceitos: parear e controlar](#conceitos-parear-e-controlar) |
| Controlar um Mac a partir do Windows | [Windows controla macOS](#windows-controla-macos) |
| Controlar um Windows a partir do Mac | [macOS controla Windows](#macos-controla-windows) |
| Controlar um Linux a partir do Windows | [Windows controla Linux](#windows-controla-linux) |
| Controlar um Linux a partir do Mac | [macOS controla Linux](#macos-controla-linux) |
| Controlar um Windows ou Mac a partir do Linux | [Linux controla Windows ou macOS](#linux-controla-windows-ou-macos-não-suportado) |

### Quem pode controlar quem

Linhas são o computador **controlador** (cujo teclado e mouse você usa); colunas, o **controlado**.

| Controlador ↓ / Controlado → | Windows | macOS | Linux |
|---|---|---|---|
| **Windows** | sim | sim | **sim** (este fork) |
| **macOS** | sim | sim | **sim** (este fork, não testado) |
| **Linux** | **não** | **não** | não |

Linux só **recebe**: este fork não captura o teclado e o mouse do Linux.

## Download

Todos os arquivos ficam na [página de Releases](https://github.com/sidneyjfg/oink/releases/latest),
sem clonar nada:

| Sistema | Arquivo |
|---|---|
| Linux x86_64 (receptor) | `oink-agent-linux-x86_64.tar.gz` |
| Windows 10/11 x64 | `Oink-0.3.7-windows-x64.msi` |
| macOS Apple silicon (M1 e mais novos) | `Oink-0.3.7-macos-arm64.dmg` |
| macOS Intel | `Oink-0.3.7-macos-x64.dmg` |

`SHA256SUMS` traz os hashes. Os arquivos de Windows e macOS são os instaladores **oficiais** do
Oink 0.3.7, sem alteração (conferidos contra o hash do projeto original ao montar a Release): o fork
só muda o Linux.

## Instalar no Windows

Requisito: Windows 10 (1809 ou mais novo) ou 11, x64.

**Opção A, pelo navegador:** baixe o [`Oink-0.3.7-windows-x64.msi`](https://github.com/sidneyjfg/oink/releases/latest)
e dê dois cliques.

**Opção B, por script** (baixa, confere o SHA-256 e instala), sem clonar o repositório:

```powershell
irm https://raw.githubusercontent.com/sidneyjfg/oink/linux-port/installers/windows/install.ps1 | iex
```

> O instalador não é assinado. Se o **Controle de Aplicativos Inteligente** (Smart App Control)
> estiver ligado, o Windows bloqueia a instalação ("a DLL necessária não pôde ser executada"). Só dá
> para desligá-lo e, em geral, sem volta: a decisão é sua, o script apenas avisa. Se aparecer o
> SmartScreen, use *Mais informações → Executar assim mesmo*.

## Instalar no macOS

Requisito: macOS 14.2 ou mais novo.

1. Baixe na [Release](https://github.com/sidneyjfg/oink/releases/latest) o
   `Oink-0.3.7-macos-arm64.dmg` (Mac com chip Apple) ou o `Oink-0.3.7-macos-x64.dmg` (Mac Intel).
2. Abra o `.dmg` e arraste o Oink para *Aplicativos*.
3. Abra o Oink. Ele pede permissões do sistema (**Acessibilidade**, **Monitoramento de Entrada** e
   rede local); conceda em *Ajustes do Sistema → Privacidade e Segurança*. Sem elas o Mac não
   consegue capturar nem receber teclado e mouse.
4. Se o macOS bloquear a abertura, vá em *Ajustes do Sistema → Privacidade e Segurança* e use
   *Abrir mesmo assim*.

## Instalar no Linux

Requisitos: Debian/Ubuntu ou derivado, 64 bits (x86_64), sessão **Xorg** (na tela de login escolha
"... on Xorg"; Wayland não funciona), `systemd`, `python3`, `libasound2` e `libssl3`.

```sh
curl -fsSL https://raw.githubusercontent.com/sidneyjfg/oink/linux-port/installers/linux/install.sh | bash
```

O script baixa o binário da [Release](https://github.com/sidneyjfg/oink/releases/latest), confere o
SHA-256 e instala em `~/.local/bin`, **sem root e sem Rust**. Ele cria o serviço `oink-agent` do
systemd de usuário, que sobe no login e acha sozinho o `DISPLAY`.

```sh
systemctl --user status oink-agent        # está rodando?
journalctl --user -u oink-agent -f        # logs
~/.local/bin/oink-agent status            # estado do agente
./installers/linux/install.sh --uninstall # remover (mantém o pareamento)
```

Sem Release, use um binário que você compilou: `./install.sh --binary caminho/oink-agent`
(veja [Compilar](#compilar-do-código-fonte)). O Linux não tem janela: tudo é por linha de comando.

## Conceitos: parear e controlar

O Oink é **peer-to-peer**: não existe "servidor" nem "cliente" fixos. Os computadores se descobrem
na rede local (todos na mesma rede) e **se pareiam uma única vez**. "Controlador" é simplesmente o
computador em cujo teclado e mouse você está mexendo; "controlado" é o que recebe.

**Parear (uma vez por par de computadores):**

1. Abra o Oink nos dois. Na janela, a lista **Dispositivos próximos** mostra o outro computador.
2. Em um deles, clique no outro. Uma tela mostra um **código de 6 dígitos**.
3. No outro computador, aceite o pedido e digite o código (no Linux, use `oink-pair`, descrito abaixo).
4. Pronto: o pareamento fica salvo.

**Controlar:** na tela de organização das telas (*Display arrangement*) arraste o ícone do outro
computador para o lado do seu, como se fosse um segundo monitor. Depois, empurre o mouse contra essa
borda: o cursor passa para o outro computador e o teclado vai junto. Também existe um atalho para
alternar a posição do ponteiro entre os computadores. (Os nomes exatos dos menus podem variar um pouco
entre versões.)

**Socorro:** se algo travar ou você perder o cursor, **segure `Esc` por 5 segundos** no computador
controlador para pausar o compartilhamento.

**Área de transferência:** o texto copiado em um computador pode ser colado no outro, nos dois
sentidos. Com Linux no meio, só texto simples (veja [Limitações](#limitações-conhecidas)).

## Windows controla macOS

*Controlador: Windows. Controlado: Mac. Os dois usam o app oficial.*

1. [Instale no Windows](#instalar-no-windows) e [no macOS](#instalar-no-macos), na mesma rede.
2. Conceda as permissões no Mac (passo 3 da instalação).
3. No Windows, abra o Oink, clique no Mac em *Dispositivos próximos* e anote o código de 6 dígitos.
   No Mac, aceite o pedido e digite o código.
4. No Windows, em *organização das telas*, coloque o Mac ao lado e empurre o mouse na borda.

## macOS controla Windows

*Controlador: Mac. Controlado: Windows. Os dois usam o app oficial.*

1. [Instale no macOS](#instalar-no-macos) e [no Windows](#instalar-no-windows), na mesma rede.
2. No Mac, conceda as permissões pedidas.
3. No Mac, abra o Oink e clique no Windows em *Dispositivos próximos*; digite o código mostrado na
   tela do Windows. (Quem inicia o pareamento tanto faz: o pareamento vale para os dois sentidos.)
4. No Mac, em *organização das telas*, coloque o Windows ao lado e empurre o mouse na borda.

## Windows controla Linux

*Controlador: Windows (app oficial). Controlado: Linux (este fork). É o caso testado e confirmado.*

1. [Instale no Linux](#instalar-no-linux) e [no Windows](#instalar-no-windows), na mesma rede, e
   mantenha a sessão gráfica do Linux aberta (logado em Xorg).
2. No Windows, abra o Oink e clique no notebook em *Dispositivos próximos*. O Windows mostra o código
   de 6 dígitos.
3. No Linux, rode: `oink-pair 123456` (com o código da tela do Windows).
4. No Windows, em *organização das telas*, coloque o notebook ao lado e empurre o mouse na borda.
   Teclado, rolagem, cliques e texto copiado passam para o Linux.

Se `oink-pair` não for encontrado, o `~/.local/bin` não está no `PATH`; use o caminho completo.

## macOS controla Linux

*Controlador: Mac (app oficial). Controlado: Linux (este fork).
O protocolo é o mesmo do Windows, mas **este caso ainda não foi testado**.*

1. [Instale no Linux](#instalar-no-linux) e [no macOS](#instalar-no-macos), na mesma rede, com as
   permissões do Mac concedidas.
2. No Mac, clique no notebook em *Dispositivos próximos* e anote o código de 6 dígitos.
3. No Linux, rode `oink-pair <código>`.
4. No Mac, em *organização das telas*, coloque o notebook ao lado e empurre o mouse na borda.

## Linux controla Windows ou macOS (não suportado)

Este fork **não captura** o teclado e o mouse do Linux, então ele não consegue controlar outro
computador (nem Windows, nem macOS). Para isso seria preciso implementar a captura de entrada via
XInput2 e o modo "grab" no `oink-agent`, o que não foi feito. Enquanto isso, o Linux só pode ser o
**controlado**: use o teclado e o mouse do Windows ou do Mac para controlá-lo
([Windows controla Linux](#windows-controla-linux), [macOS controla Linux](#macos-controla-linux)).

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
