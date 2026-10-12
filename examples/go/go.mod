module github.com/mcpd/mcpd/examples/go

go 1.21

require github.com/mcpd/mcpd/sdks/go v0.0.0

// The SDK lives in this repository, so the example builds against the working
// tree rather than a published version.
replace github.com/mcpd/mcpd/sdks/go => ../../sdks/go
