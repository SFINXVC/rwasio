#!/bin/bash
SPEC_FILE="$(cd "$(dirname "$0")" && pwd)/rwasio.dll.spec"

# Extract output filename
OUTPUT_FILE=""
PREV=""
for arg in "$@"; do
    if [[ "$PREV" == "-o" ]]; then
        OUTPUT_FILE="$arg"
    fi
    PREV="$arg"
done

IS_SHARED=0
for arg in "$@"; do
    [[ "$arg" == "-shared" ]] && IS_SHARED=1
done

# Only use winegcc for our actual .dll.so — proc macros and other shared libs use gcc
if [[ $IS_SHARED -eq 1 && "$OUTPUT_FILE" == *.dll.so ]]; then
    FILTERED=()
    SKIP_NEXT=0
    for arg in "$@"; do
        if [[ $SKIP_NEXT -eq 1 ]]; then
            SKIP_NEXT=0
            continue
        fi
        case "$arg" in
            --version-script=*|-Wl,--version-script=*)  ;;
            --no-undefined-version|-Wl,--no-undefined-version) ;;
            --gc-sections|-Wl,--gc-sections) ;;
            --no-undefined|-Wl,--no-undefined) ;;
            -nodefaultlibs) ;;
            -pie) ;;
            -z) SKIP_NEXT=1 ;;
            *) FILTERED+=("$arg") ;;
        esac
    done
    winegcc -m64 "${FILTERED[@]}" "$SPEC_FILE" $(pkg-config --libs gtk4 libadwaita-1 libpipewire-0.3)
else
    GCC_ARGS=()
    SKIP_NEXT=0
    for arg in "$@"; do
        if [[ $SKIP_NEXT -eq 1 ]]; then
            GCC_ARGS+=("-Wl,$arg")
            SKIP_NEXT=0
            continue
        fi
        case "$arg" in
            --as-needed|--no-as-needed|--eh-frame-hdr|\
            --gc-sections|--no-undefined|--no-undefined-version|\
            --allow-multiple-definition|--strip-all)
                GCC_ARGS+=("-Wl,$arg") ;;
            --version-script=*|--build-id*|--hash-style*|--dynamic-linker*)
                GCC_ARGS+=("-Wl,$arg") ;;
            -z)
                GCC_ARGS+=("-Wl,-z")
                SKIP_NEXT=1 ;;
            *)
                GCC_ARGS+=("$arg") ;;
        esac
    done
    exec gcc "${GCC_ARGS[@]}"
fi