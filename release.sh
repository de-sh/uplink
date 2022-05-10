#!/bin/sh
TARGETS=$@
BUILD_DIR="build/"

if test -e /var/run/docker.sock; then
    echo "Docker daemon volume is attached!"
else
    echo "Volume is not attached! Please attach it first, then re-run this script!"
    exit 1
fi

for TARGET in ${TARGETS}
do
    echo "Building uplink for ${TARGET}"
    cross build --release --target ${TARGET}
done

echo "Creating directory to store built executables: ${BUILD_DIR}"
mkdir -p ${BUILD_DIR}
for TARGET in ${TARGETS}
do
    echo "uplink binary for ${TARGET} copied into build/ folder"
    cp target/${TARGET}/release/uplink ${BUILD_DIR}uplink-${TARGET}
done
