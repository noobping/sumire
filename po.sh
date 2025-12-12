#!/bin/sh
APP_ID=io.github.noobping.listenmoe

for f in po/*.po; do
    lang=$(basename "$f" .po)
    mkdir -p "data/locale/$lang/LC_MESSAGES"
    msgfmt "$f" -o "data/locale/$lang/LC_MESSAGES/$APP_ID.mo"
    msgfmt "$f" -o "data/locale/$lang/LC_MESSAGES/$APP_ID_develop.mo"
done

for f in po/*.po; do lang=$(basename "$f" .po)
    mkdir -p "AppDir/share/locale/$lang/LC_MESSAGES"
    msgfmt "$f" -o "AppDir/share/locale/$lang/LC_MESSAGES/$APP_ID.mo"
done
