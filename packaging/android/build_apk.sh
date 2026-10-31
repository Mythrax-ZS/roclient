#!/bin/sh
# Packs the Android build into an APK without Gradle: the app is a NativeActivity with no Java
# code, so aapt2 (manifest, icon, assets), zip (the native library), zipalign and apksigner
# from the SDK's build-tools are all it takes.
#   packaging/android/build_apk.sh <dir holding lib/arm64-v8a/libtenseiga_android.so> <out.apk>
# Needs ANDROID_HOME. VERSION_CODE / VERSION_NAME set the version (CI: the run number).
# Signing: ANDROID_KEYSTORE (path to a PKCS#12 file) and ANDROID_KEYSTORE_PASS, else the
# sideload key in this folder, which keeps every build installable over the last one.
set -eu
libs="$1"
out="$2"
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../.."
bt="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
platform="$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The Samurai overlay, unpacked by the app on start (lib.rs) from the list in index.txt.
mkdir -p "$work/assets/overlays"
cp -r "$root/overlays/samurai" "$work/assets/overlays/"
(cd "$work/assets" && find overlays -type f ! -name index.txt | sort > overlays/index.txt)

"$bt/aapt2" compile --dir "$here/res" -o "$work/res.zip"
"$bt/aapt2" link -o "$work/base.apk" \
    --manifest "$here/AndroidManifest.xml" \
    -I "$platform/android.jar" \
    -A "$work/assets" \
    --min-sdk-version 33 --target-sdk-version 35 \
    --version-code "${VERSION_CODE:-1}" --version-name "${VERSION_NAME:-dev}" \
    "$work/res.zip"
(cd "$libs" && zip -qr "$work/base.apk" lib)
"$bt/zipalign" -f 4 "$work/base.apk" "$work/aligned.apk"
"$bt/apksigner" sign \
    --ks "${ANDROID_KEYSTORE:-$here/sideload.p12}" --ks-type PKCS12 \
    --ks-pass "pass:${ANDROID_KEYSTORE_PASS:-tenseiga}" \
    --out "$out" "$work/aligned.apk"
"$bt/apksigner" verify "$out"
ls -l "$out"
