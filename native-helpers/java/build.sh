#!/bin/sh
# 仅编译 Narrant helper 自身；不会读取或执行目标项目构建逻辑。
set -eu
if [ "$#" -ne 2 ]; then
  echo 'usage: build.sh JDK_HOME LOCKED_LIB_DIRECTORY' >&2
  exit 2
fi
JDK_HOME=$1
LIB_DIRECTORY=$2
SCRIPT_DIRECTORY=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BUILD_DIRECTORY="$SCRIPT_DIRECTORY/build/classes"
mkdir -p "$BUILD_DIRECTORY"
CLASSPATH=$(python3 "$SCRIPT_DIRECTORY/tools/classpath.py" "$LIB_DIRECTORY")
"$JDK_HOME/bin/javac" -encoding UTF-8 -proc:none -cp "$CLASSPATH" -d "$BUILD_DIRECTORY" \
  "$SCRIPT_DIRECTORY"/src/dev/narrant/java/*.java
echo "$BUILD_DIRECTORY"
