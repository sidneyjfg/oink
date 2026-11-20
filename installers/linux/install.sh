#!/usr/bin/env bash
# Instala o oink-agent (receptor Linux/X11) como serviço de usuário do systemd.
#
#   ./install.sh                      baixa a última versão das Releases e instala
#   ./install.sh --version v0.3.7-linux1
#   ./install.sh --binary ./oink-agent  usa um binário que você já tem
#   ./install.sh --uninstall          remove binário e serviço (mantém os dados e o pareamento)
#
# Não precisa de root nem de Rust. Instala em ~/.local (mude com PREFIX=...).
set -euo pipefail

REPO="${OINK_REPO:-sidneyjfg/oink}"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
DATA_DIR="${OINK_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/oink}"
ASSET="oink-agent-linux-x86_64.tar.gz"

version="latest"
binary=""
uninstall=0
while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="$2"; shift 2 ;;
    --binary) binary="$2"; shift 2 ;;
    --uninstall) uninstall=1; shift ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "opção desconhecida: $1" >&2; exit 2 ;;
  esac
done

have() { command -v "$1" >/dev/null 2>&1; }
die() { echo "erro: $*" >&2; exit 1; }

if [ "$uninstall" = 1 ]; then
  systemctl --user disable --now oink-agent.service 2>/dev/null || true
  rm -f "$UNIT_DIR/oink-agent.service" "$BIN_DIR/oink-agent" "$BIN_DIR/oink-agent-launch" "$BIN_DIR/oink-pair"
  systemctl --user daemon-reload 2>/dev/null || true
  echo "Removido. Dados e pareamento continuam em $DATA_DIR (apague a pasta para esquecer o pareamento)."
  exit 0
fi

[ "$(uname -s)" = Linux ] || die "só funciona em Linux"
[ "$(uname -m)" = x86_64 ] || die "só há binário para x86_64 (você tem $(uname -m)); compile do código-fonte"
have systemctl || die "systemd é necessário"
have python3 || die "python3 é necessário (usado pelo oink-pair)"
ldconfig -p 2>/dev/null | grep -q 'libasound\.so\.2' || echo "aviso: libasound2 não encontrada (sudo apt install libasound2)" >&2
ldconfig -p 2>/dev/null | grep -q 'libssl\.so\.3' || echo "aviso: libssl3 não encontrada (sudo apt install libssl3)" >&2

mkdir -p "$BIN_DIR" "$UNIT_DIR" "$DATA_DIR"

if [ -z "$binary" ]; then
  have curl || die "curl é necessário para baixar"
  tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
  if [ "$version" = latest ]; then
    base="https://github.com/$REPO/releases/latest/download"
  else
    base="https://github.com/$REPO/releases/download/$version"
  fi
  echo "Baixando $base/$ASSET"
  curl -fL --proto '=https' -o "$tmp/$ASSET" "$base/$ASSET" || die "download falhou (há uma Release com $ASSET?)"
  curl -fL --proto '=https' -o "$tmp/$ASSET.sha256" "$base/$ASSET.sha256" || die "não achei $ASSET.sha256; recuso instalar sem conferir"
  (cd "$tmp" && sha256sum -c "$ASSET.sha256") || die "hash não confere"
  tar -xzf "$tmp/$ASSET" -C "$tmp"
  binary="$tmp/oink-agent"
fi
[ -f "$binary" ] || die "binário não encontrado: $binary"

# Para o serviço antes de trocar o arquivo; ele é religado no fim.
systemctl --user stop oink-agent.service 2>/dev/null || true
install -m 0755 "$binary" "$BIN_DIR/oink-agent"

# Descobre a sessão gráfica (DISPLAY e XAUTHORITY) na hora de subir: o serviço pode
# iniciar antes do login, então sai com erro e o systemd tenta de novo.
cat > "$BIN_DIR/oink-agent-launch" <<'EOF'
#!/bin/sh
bin="$(dirname "$0")/oink-agent"
if [ -z "${DISPLAY:-}" ]; then
  sid="$(loginctl show-user "$(id -un)" -p Display --value 2>/dev/null)"
  [ -n "$sid" ] && DISPLAY="$(loginctl show-session "$sid" -p Display --value 2>/dev/null)"
fi
[ -n "${DISPLAY:-}" ] || { echo "oink: sem sessão X11 ativa ainda" >&2; exit 1; }
export DISPLAY
if [ -z "${XAUTHORITY:-}" ]; then
  for f in "/run/user/$(id -u)/gdm/Xauthority" "$HOME/.Xauthority"; do
    [ -r "$f" ] && XAUTHORITY="$f" && break
  done
fi
[ -n "${XAUTHORITY:-}" ] && export XAUTHORITY
exec "$bin" run --home "${OINK_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/oink}"
EOF
chmod 0755 "$BIN_DIR/oink-agent-launch"

# Pareamento sem janela: o PC mostra um código de 6 dígitos; digite-o aqui.
cat > "$BIN_DIR/oink-pair" <<'EOF'
#!/bin/sh
# Uso: oink-pair 123456   (antes, no PC, clique neste notebook em "Dispositivos próximos")
set -e
code="${1:?uso: oink-pair <código de 6 dígitos mostrado no PC>}"
case "$code" in [0-9][0-9][0-9][0-9][0-9][0-9]) ;; *) echo "o código tem 6 dígitos" >&2; exit 2 ;; esac
a="$(dirname "$0")/oink-agent"
h="${OINK_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/oink}"
py='import sys,json
d=json.load(sys.stdin); kind=sys.argv[1]
if kind=="request":
    r=d["requests"]; print(r[0]["id"],r[0]["revision"]) if r else sys.exit(1)
else:
    s=[x for x in d["sessions"] if x["direction"]=="incoming" and x["phase"]==2]; print(s[0]["id"],s[0]["revision"]) if s else sys.exit(1)'
set -- $("$a" --home "$h" pairing | python3 -c "$py" request) || { echo "nenhum pedido do PC; clique neste notebook no Oink do PC e tente de novo" >&2; exit 1; }
"$a" --home "$h" call accept_pairing "{\"pair_id\":\"$1\",\"expected_revision\":\"$2\"}" >/dev/null
sleep 1
set -- $("$a" --home "$h" pairing | python3 -c "$py" session) || { echo "o pedido foi aceito, mas a sessão não apareceu; tente de novo" >&2; exit 1; }
"$a" --home "$h" code "$1" "$2" 1 "$code"
EOF
chmod 0755 "$BIN_DIR/oink-pair"

cat > "$UNIT_DIR/oink-agent.service" <<EOF
[Unit]
Description=Oink agent (teclado, mouse e área de transferência compartilhados)
After=graphical-session.target network-online.target

[Service]
Environment=OINK_HOME=$DATA_DIR
ExecStart=$BIN_DIR/oink-agent-launch
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now oink-agent.service

echo
echo "Instalado em $BIN_DIR. Serviço: systemctl --user status oink-agent"
echo "Próximo passo: no Oink do PC clique neste notebook e rode aqui:  $BIN_DIR/oink-pair <código>"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "dica: coloque $BIN_DIR no PATH" ;; esac
