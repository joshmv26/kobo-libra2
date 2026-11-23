#! /bin/sh

if [ "$#" -lt 1 ] ; then
	printf "Usage: %s NICKEL_MENU_ARCHIVE\n" "${0##*/}" >&2
	exit 1
fi

[ -d dist ] || ./dist.sh
[ -d bundle ] && rm -Rf bundle

NICKEL_MENU_ARCHIVE=$1

mkdir bundle
cd bundle || exit 1

if gzip -tq "$NICKEL_MENU_ARCHIVE"; then
	cp "$NICKEL_MENU_ARCHIVE" KoboRoot.tgz
else
	unzip "$NICKEL_MENU_ARCHIVE" KoboRoot.tgz
fi

mkdir -p .adds/nm .kobo
# keep the original NickelMenu KoboRoot.tgz byte-for-byte (repacking changed file owners)
mv KoboRoot.tgz .kobo/KoboRoot.tgz
mv ../dist .adds/plato
cp ../contrib/NickelMenu/* .adds/nm/
cp ../contrib/Plato/home.toml .adds/plato/home.toml
cp ../contrib/Plato/telegram.toml .adds/plato/telegram.toml
tar -xzOf .kobo/KoboRoot.tgz ./mnt/onboard/.adds/nm/doc > .adds/nm/doc 2>/dev/null || true

FIRMWARE_VERSION=$(basename "$FIRMWARE_ARCHIVE" .zip)
FIRMWARE_VERSION=${FIRMWARE_VERSION##*-}
PLATO_VERSION=$(cargo pkgid -p plato | cut -d '#' -f 2)

zip -r plato-bundle-"$PLATO_VERSION".zip .adds .kobo
rm -Rf .adds .kobo
