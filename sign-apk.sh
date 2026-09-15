#!/bin/bash

# export RANLIB=$ANDROID_HOME/ndk/26.1.10909125/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ranlib && cargo tauri android build --target aarch64 2>&1 | tail -30
#
# APK 签名脚本

KEYSTORE="xchat-release.keystore"
KEYSTORE_ALIAS="xchat"
APK_UNSIGNED="src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"
APK_SIGNED="xchat-aarch64.apk"

echo "=== Xchat APK 签名工具 ==="
echo ""

# 密钥库必须存在，缺失时直接失败。
#
# 这里以前会自动新建一个。那是个陷阱：Android 只允许同一签名的 APK 覆盖安装，
# 换台机器构建时静默生成新密钥，装到设备上就会 INSTALL_FAILED_UPDATE_INCOMPATIBLE，
# 只能卸载重装、清掉本地数据。宁可在这里停下来，也不要无声地轮换掉签名密钥。
#
# 生产密钥库不在仓库里（.gitignore 已排除 *.keystore），需要向项目负责人索取，
# 放到仓库根目录并保持文件名不变。
if [ ! -f "$KEYSTORE" ]; then
    echo "❌ 找不到签名密钥库: $KEYSTORE"
    echo ""
    echo "   这个文件不在仓库里，需要单独获取后放到仓库根目录。"
    echo "   不要为了让它跑起来而新建密钥 —— 新密钥签出的 APK 无法覆盖安装，"
    echo "   用户必须卸载重装并丢失本地数据。"
    echo ""
    echo "   如果你确实是在初始化一个全新的发布渠道（没有存量用户），"
    echo "   再手动执行下面这条命令："
    echo ""
    echo "   keytool -genkey -v -keystore $KEYSTORE -alias $KEYSTORE_ALIAS \\"
    echo "     -keyalg RSA -keysize 2048 -validity 10000 \\"
    echo "     -storepass android -keypass android \\"
    echo "     -dname \"CN=Xchat, OU=Dev, O=Xchat, L=City, S=State, C=CN\""
    echo ""
    exit 1
fi

# 检查 unsigned APK 是否存在
if [ ! -f "$APK_UNSIGNED" ]; then
    echo "❌ 找不到未签名的 APK: $APK_UNSIGNED"
    echo "请先运行: cargo tauri android build"
    exit 1
fi

echo "正在签名 APK..."
echo "输入文件: $APK_UNSIGNED"
echo "输出文件: $APK_SIGNED"
echo ""

# 使用 apksigner 签名（Android SDK 自带）
if [ -z "$ANDROID_HOME" ]; then
    echo "❌ 环境变量 ANDROID_HOME 未设置，找不到 Android SDK"
    exit 1
fi

BUILD_TOOLS="$ANDROID_HOME/build-tools/$(ls "$ANDROID_HOME/build-tools" 2>/dev/null | sort -V | tail -1)"
# Windows 上 SDK 只提供 apksigner.bat，没有无后缀的 apksigner
if [ -f "$BUILD_TOOLS/apksigner" ]; then
    APKSIGNER="$BUILD_TOOLS/apksigner"
elif [ -f "$BUILD_TOOLS/apksigner.bat" ]; then
    APKSIGNER="$BUILD_TOOLS/apksigner.bat"
else
    echo "❌ 找不到 apksigner 工具（找过 $BUILD_TOOLS）"
    echo "请确保已安装 Android SDK build-tools"
    exit 1
fi

# 签名 APK
$APKSIGNER sign \
    --ks "$KEYSTORE" \
    --ks-key-alias "$KEYSTORE_ALIAS" \
    --ks-pass pass:android \
    --key-pass pass:android \
    --out "$APK_SIGNED" \
    "$APK_UNSIGNED"

if [ $? -ne 0 ]; then
    echo "❌ APK 签名失败"
    exit 1
fi

echo ""
echo "✓ APK 签名成功！"
echo ""
echo "签名后的 APK: $APK_SIGNED"
echo "文件大小: $(du -h $APK_SIGNED | cut -f1)"
echo ""
echo "现在可以安装到设备："
echo "  adb install $APK_SIGNED"
echo ""
echo "或者直接推送到设备："
echo "  adb push $APK_SIGNED /sdcard/Download/"
