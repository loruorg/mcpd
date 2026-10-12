plugins {
    kotlin("jvm") version "2.4.20"
    application
}

repositories {
    mavenCentral()
}

dependencies {
    implementation("dev.mcpd:mcpd-kotlin:0.1.0")
}

kotlin {
    jvmToolchain(17)
}

// The example is one file at the root of this directory, so the docs can
// include it by path without a src/main/kotlin prefix in every link.
sourceSets {
    main {
        kotlin.setSrcDirs(listOf(projectDir))
        kotlin.exclude("build/**", ".gradle/**", "*.gradle.kts")
    }
}

application {
    mainClass.set("examples.QuickstartKt")
}
