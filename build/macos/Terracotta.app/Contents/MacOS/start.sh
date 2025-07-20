#!/bin/bash

# Architecture Detection
echo "Checking Architecture ..."

# 获取脚本所在的目录
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)

# 二进制文件名定义
x86_64_bin="terracotta-macos-x86_64"
arm64_bin="terracotta-macos-arm64"

# 检测架构
ARCH=$(uname -m)
case "$ARCH" in
    "x86_64")
        echo "Detected x86_64 architecture"
        run_bin="$x86_64_bin"
        ;;
    "arm64"|"aarch64")
        echo "Detected ARM64 architecture"
        run_bin="$arm64_bin"
        ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# 切换到脚本目录并执行对应二进制
cd "$SCRIPT_DIR" || exit 1
chmod +x "$run_bin" 2>/dev/null  # 确保有执行权限
echo "Launching $run_bin ..."
./"$run_bin" "$@"

exit 0
