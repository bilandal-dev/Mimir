#!/usr/bin/env bash
# Erzeugt das Archiv, aus dem die PKGBUILD die Binary lädt.
#
# Der Ablauf ist bewusst zweistufig:
#
#  1. Hier wird die Binary gebaut und in ein Archiv gepackt. Das Archiv wird zu
#     einem Release geuploadet – zusammen mit dem Quelltext-Repository.
#  2. Die PKGBUILD in `packaging/arch/` lädt dieses Archiv herunter und
#     installiert es, ohne selbst etwas zu bauen.
#
# Der Grund für die Trennung: Mimir hängt an 143 Systembibliotheken. Ein
# AUR-Eintrag, der selbst baut, zwingt jedem Nutzer eine Rust-Toolchain und einen
# zehnminütigen Build auf. Wer ein Chat-Programm installieren will, soll dafür
# keine zwanzig Minuten warten.
#
# Aufruf:
#   ./packaging/arch/erzeuge-archiv.sh
#   ./packaging/arch/erzeuge-archiv.sh 0.2.0
#
# Ergebnis:
#   packaging/arch/mimir-<version>-x86_64.tar.gz

set -euo pipefail

VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)}"
ARCH="x86_64"
WURZEL="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ZIEL="$WURZEL/packaging/arch"
ARCHIV="$ZIEL/mimir-$VERSION-$ARCH.tar.gz"

if [ -z "$VERSION" ] || [ "$VERSION" = "0.0.0" ]; then
  echo "FEHLER: Version nicht ermittelt. Aufruf mit einer Version:" >&2
  echo "        ./packaging/arch/erzeuge-archiv.sh 0.1.0" >&2
  exit 1
fi

echo "==> Version $VERSION, Architektur $ARCH"

# 1. Binary bauen. Ohne Bundle-Ziel: Das erzeugt nur die Binary und lädt keine
#    Werkzeuge für AppImage oder Windows-Installer nach.
echo "==> Binary bauen (das dauert ein paar Minuten)"
( cd "$WURZEL/src-tauri" && cargo tauri build --no-bundle )

BINARY="$WURZEL/src-tauri/target/release/mimir"
if [ ! -x "$BINARY" ]; then
  echo "FEHLER: $BINARY wurde nicht erzeugt" >&2
  exit 1
fi

# 2. Prüfungen, bevor etwas hochgeladen wird. Ein Archiv mit fremden Adressen
#    darin zu veröffentlichen wäre der schlimmste der Fehler – die landen sonst
#    für immer im Release.
echo "==> Binary prüfen"
node "$WURZEL/src/tests/binary-pruefen.mjs" "$BINARY"

# 3. Desktop-Datei. Tauri erzeugt sie nur beim Bundle-Bau, und der Text muss zur
#    Beschreibung in tauri.conf.json passen, sonst beschwert sich `desktop-file-validate`.
KURZTEXT=$(sed -n 's/.*"shortDescription": "\(.*\)".*/\1/p' "$WURZEL/src-tauri/tauri.conf.json")
cat > "$ZIEL/mimir.desktop" <<EOF
[Desktop Entry]
Categories=Development;
Comment=$KURZTEXT
Exec=mimir
StartupWMClass=mimir
Icon=mimir
Name=Mimir
Terminal=false
Type=Application
EOF

if command -v desktop-file-validate >/dev/null 2>&1; then
  echo "==> Desktop-Datei prüfen"
  desktop-file-validate "$ZIEL/mimir.desktop"
fi

# 4. Lizenz. `license = "MIT"` im Manifest ist eine Behauptung; das fertige
#    Paket braucht die Datei daneben, sonst nennt es keine Lizenz.
LICENSE="$WURZEL/LICENSE"
if [ ! -f "$LICENSE" ]; then
  echo "FEHLER: $LICENSE fehlt." >&2
  echo "        Ohne Lizenzdatei darf nichts veröffentlicht werden – weder in" >&2
  echo "        das Archiv noch in ein Repository. Siehe docs/entwicklung.md." >&2
  exit 1
fi
cp "$LICENSE" "$ZIEL/mimir.license"

# `mimir.desktop` wird hier nur als Zwischenprodukt erzeugt und nicht committet:
# Es steht im Archiv, und im Repository wäre es eine zweite Wahrheit, die auseinander
# laufen kann. Neu erzeugen: ./packaging/arch/erzeuge-archiv.sh


# 5. Archiv bauen. Feste Reihenfolge und fester Besitzer, damit zwei Builds
#    desselben Standes dieselbe Prüfsumme ergeben.
# Das Icon wird nur zum Packen dorthin kopiert und danach wieder entfernt: Es
# gehört nicht neben die PKGBUILD ins Arbeitsverzeichnis, sondern ins Archiv.
cp "$WURZEL/src-tauri/icons/128x128.png" "$ZIEL/mimir.png"

echo "==> Archiv bauen"
rm -f "$ARCHIV"
tar --sort=name \
    --owner=0 --group=0 --numeric-owner \
    --mtime='UTC 2020-01-01' \
    -czf "$ARCHIV" \
    -C "$ZIEL" mimir.desktop mimir.license mimir.png \
    -C "$(dirname "$BINARY")" mimir
rm -f "$ZIEL/mimir.png"

echo
echo "==> Fertig: $ARCHIV"
echo "    $(du -h "$ARCHIV" | cut -f1)"
echo
echo "Nächste Schritte:"
echo "  1. Prüfsumme in packaging/arch/PKGBUILD eintragen:"
echo "       sha256sum -b $ARCHIV"
echo "  2. Archiv als Release hochladen (gleicher Name wie hier)."
echo "  3. In der PKGBUILD die Release-Adresse anpassen."
echo "  4. Prüfen:  cd packaging/arch && makepkg -si"
