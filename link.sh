#!/bin/bash
SPEC_FILE="$(cd "$(dirname "$0")" && pwd)/rwasio.dll.spec"

IS_SHARED=0
for arg in "$@"; do
    [[ "$arg" == "-shared" ]] && IS_SHARED=1
done

if [[ $IS_SHARED -eq 0 ]]; then
    exec gcc "$@"
fi

FILTERED=()
SKIP_NEXT=0
for arg in "$@"; do
    if [[ $SKIP_NEXT -eq 1 ]]; then
        SKIP_NEXT=0
        continue
    fi
    case "$arg" in
        --version-script=*|-Wl,--version-script=*);;
        --no-undefined-version|-Wl,--no-undefined-version);;
        --gc-sections|-Wl,--gc-sections);;
        --no-undefined|-Wl,--no-undefined);;
        -nodefaultlibs);;
        -pie);;
        -z) SKIP_NEXT=1;;
        *) FILTERED+=("$arg");;
    esac
done

winegcc -m64 "${FILTERED[@]}" "$SPEC_FILE"