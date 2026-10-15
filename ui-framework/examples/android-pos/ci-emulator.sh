#!/usr/bin/env bash
# Runs inside android-emulator-runner: installs the app, captures a launch
# screenshot, runs the instrumented sale test and collects evidence.
set -euo pipefail
OUT=build/emulator-evidence
mkdir -p "$OUT"
adb shell getprop ro.product.model | tee "$OUT/device.txt"
adb shell getprop ro.build.version.sdk | tee -a "$OUT/device.txt"
adb shell getprop ro.product.cpu.abi | tee -a "$OUT/device.txt"
adb shell wm size | tee -a "$OUT/device.txt"
adb shell wm density | tee -a "$OUT/device.txt"
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb install -r app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb logcat -c
adb shell am start -W -n dev.kite.pos/.MainActivity
sleep 6
adb shell screencap -p /data/local/tmp/kite-0-launch.png
adb shell am instrument -w dev.kite.pos.test/androidx.test.runner.AndroidJUnitRunner | tee "$OUT/instrument.txt"
adb shell 'ls /data/local/tmp/kite-*.png' | tr -d '\r' | while read -r f; do adb pull "$f" "$OUT/" >/dev/null; done
adb logcat -d -v time > "$OUT/logcat.txt"
echo "---- KitePOS / crash lines from logcat ----"
grep -E "KitePOS|kitepos|FATAL EXCEPTION|AndroidRuntime" "$OUT/logcat.txt" | tail -60 || true
ls -la "$OUT"
grep -q "OK (1 test)" "$OUT/instrument.txt"
