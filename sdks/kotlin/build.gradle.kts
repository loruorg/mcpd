import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

plugins {
    `java-library`
    `maven-publish`
    signing
    kotlin("jvm") version "2.4.21"
    kotlin("plugin.serialization") version "2.4.21"
}

group = "dev.mcpd"
version = "0.1.0"

repositories {
    mavenCentral()
}

dependencies {
    // The generated types are part of the public API, so serialization is
    // `api`: a caller that reads a Trace needs it on its own compile
    // classpath. Timestamps are `kotlin.time.Instant` from the standard
    // library, which kotlinx.serialization 1.9 and later serialise on their
    // own, so kotlinx-datetime is no longer a dependency.
    api("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    api("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")

    testImplementation(kotlin("test"))
}

kotlin {
    jvmToolchain(17)
}

// Maven Central refuses a release without a sources jar and a javadoc jar
// beside the main one. The sources jar carries the KDoc, which is where this
// SDK's documentation lives. Gradle's javadoc task reads Java sources only, so
// for this all-Kotlin library it produces nothing, and the javadoc jar carries
// the README instead, which is what Central's guidance asks of a project with
// no Javadoc. A generated API site would need Dokka, which this build does not
// use.
java {
    withSourcesJar()
    withJavadocJar()
}

tasks.named<Jar>("javadocJar") {
    from("README.md")
}

tasks.withType<KotlinCompile>().configureEach {
    compilerOptions {
        // explicitNulls, used to keep null fields off the wire.
        optIn.add("kotlinx.serialization.ExperimentalSerializationApi")
    }
}

tasks.withType<Test>().configureEach {
    useJUnitPlatform()
    // The tests stand up a real HTTP server on a loopback port and assert on
    // what arrives. They need no network and no running mcpd.
    testLogging {
        events("passed", "failed", "skipped")
    }
}

// A Maven artefact states its licence in its own POM. Without this block the
// published jar carries no licence at all, whatever the repository says, so a
// consumer scanning its dependency tree sees an unlicensed artefact. The
// licence text itself ships beside this file, in sdks/kotlin/LICENSE.
publishing {
    publications {
        create<MavenPublication>("maven") {
            from(components["java"])
            pom {
                name.set("mcpd Kotlin SDK")
                description.set(
                    "Agent observability and durable runs for Kotlin and the JVM. Sends " +
                        "traces, logs, metrics, scores and feedback to an mcpd daemon, and " +
                        "serves the steps of its durable runs."
                )
                url.set("https://mcpd.sh")
                licenses {
                    license {
                        name.set("Apache License, Version 2.0")
                        url.set("https://www.apache.org/licenses/LICENSE-2.0")
                        distribution.set("repo")
                    }
                }
                organization {
                    name.set("Internet Technology Services LLC")
                    url.set("https://mcpd.sh")
                }
                // Central requires a developer entry. MAINTAINERS.md names the
                // maintainers as a team behind one address, so this does too.
                developers {
                    developer {
                        id.set("mcpd")
                        name.set("The mcpd maintainers")
                        email.set("hi@mcpd.sh")
                        organization.set("Internet Technology Services LLC")
                        organizationUrl.set("https://mcpd.sh")
                    }
                }
                scm {
                    url.set("https://github.com/mcpd/mcpd")
                    connection.set("scm:git:https://github.com/mcpd/mcpd.git")
                    developerConnection.set("scm:git:ssh://git@github.com/mcpd/mcpd.git")
                }
                issueManagement {
                    system.set("GitHub")
                    url.set("https://github.com/mcpd/mcpd/issues")
                }
            }
        }
    }
}

// Central also refuses unsigned artefacts. The key is read only when one is
// given, as the Gradle properties signingKey (an ASCII-armoured private key)
// and signingPassword, for example from ORG_GRADLE_PROJECT_signingKey in a
// release job. With no key, signing is not configured at all, so a local
// build and `publishToMavenLocal` work as before on any machine. Nothing in
// this repository publishes to Central yet: that needs an account and a key
// the maintainers have not created.
val signingKey = providers.gradleProperty("signingKey").orNull
if (!signingKey.isNullOrBlank()) {
    signing {
        useInMemoryPgpKeys(signingKey, providers.gradleProperty("signingPassword").orNull ?: "")
        sign(publishing.publications["maven"])
    }
}
