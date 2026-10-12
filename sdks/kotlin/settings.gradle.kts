// The SDK compiles for a JDK 17 toolchain, so a consumer on 17 can use it. A
// machine with only a newer JDK, such as the gradle:9.7.0-jdk21 image, would
// otherwise stop with "Cannot find a Java installation matching
// languageVersion=17". This lets Gradle download that JDK when none is
// installed; a machine that has one, as CI does, downloads nothing.
plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

rootProject.name = "mcpd-kotlin"
