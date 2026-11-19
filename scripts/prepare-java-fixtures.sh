#!/bin/sh
set -eu

repo=$(pwd -P)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

for name in java_pack java_pack_fail cov_java; do
    fixture="$repo/testdata/$name"
    work="$scratch/$name"
    mkdir -p "$work"
    cp -R "$fixture/pom.xml" "$fixture/src" "$work/"
    (
        cd "$work"
        maven() {
            mvn --batch-mode --no-transfer-progress "-Dmaven.repo.local=$fixture/.sc/m2" \
                -Dmaven.compiler.source=17 -Dmaven.compiler.target=17 "$@"
        }
        maven -DskipTests compile
        maven org.jacoco:jacoco-maven-plugin:0.8.12:prepare-agent test
        if [ -f target/jacoco.exec ]; then
            maven org.jacoco:jacoco-maven-plugin:0.8.12:report
        fi
    )
done
