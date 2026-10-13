package mcpd

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	// DefaultPromptCacheTTL is how long GetPrompt serves a fetched prompt
	// before it asks the daemon again.
	DefaultPromptCacheTTL = 60 * time.Second

	// DefaultPromptLabel is the label GetPrompt reads when it is given
	// neither a label nor a version.
	DefaultPromptLabel = "production"

	// promptCacheSize bounds the cache (B3): a process that reads prompts by
	// many names cannot grow it without limit.
	promptCacheSize = 256

	// Span attributes that link a span to the prompt it used. The daemon
	// indexes them for ListSpans(SpanQuery{Prompt, PromptVersion}).
	promptNameAttribute    = "mcpd.prompt.name"
	promptVersionAttribute = "mcpd.prompt.version"
)

// PromptOptions selects the version GetPrompt reads and says what to do when
// the daemon cannot answer. The zero value reads the label "production",
// falls back to the latest version, and caches for DefaultPromptCacheTTL.
type PromptOptions struct {
	// Project defaults to the client's project.
	Project string
	// Label reads the version this label points at. Not with Version.
	Label string
	// Version reads this version. Zero means not set, since versions start at
	// 1. Not with Label.
	Version uint32
	// CacheTTL is how long a fetched copy is served with no request. Nil
	// means DefaultPromptCacheTTL. A pointer to zero fetches every time and
	// still serves a stale copy when the fetch fails.
	CacheTTL *time.Duration
	// FallbackTemplate is returned as a text prompt when the fetch fails and
	// nothing was fetched before. Not with FallbackMessages.
	FallbackTemplate string
	// FallbackMessages is returned as a chat prompt when the fetch fails and
	// nothing was fetched before. Not with FallbackTemplate.
	FallbackMessages []map[string]any
}

// GetPrompt reads one version of a prompt, from the cache when the copy there
// is younger than CacheTTL and from the daemon otherwise.
//
// When the fetch fails and a copy was fetched before, that copy is returned
// however old it is and the failure is reported once through the client's
// logger, so a daemon outage never breaks an agent that has fetched its prompt
// once. With no copy, the fallback is returned if one was given; it is never
// cached, so the next call tries the daemon again. With neither, the fetch
// error is returned.
//
// It works on a disabled client: a prompt is the agent's input, not
// telemetry.
func (c *Client) GetPrompt(ctx context.Context, name string, opts PromptOptions) (*Prompt, error) {
	if name == "" {
		return nil, errors.New("mcpd: GetPrompt needs a prompt name")
	}
	if opts.Label != "" && opts.Version != 0 {
		return nil, errors.New("mcpd: GetPrompt takes a Label or a Version, not both")
	}
	if opts.FallbackTemplate != "" && opts.FallbackMessages != nil {
		return nil, errors.New("mcpd: GetPrompt takes a FallbackTemplate or FallbackMessages, not both")
	}
	if ctx == nil {
		ctx = context.Background()
	}
	project := opts.Project
	if project == "" && c != nil {
		project = c.cfg.project
	}
	ttl := DefaultPromptCacheTTL
	if opts.CacheTTL != nil {
		ttl = *opts.CacheTTL
	}

	key := promptCacheKey(project, name, opts)
	var cached *promptEntry
	if c != nil {
		cached = c.prompts.get(key)
		if cached != nil && ttl > 0 && time.Since(cached.fetched) < ttl {
			return clonePrompt(&cached.prompt), nil
		}
	}

	fetched, err := c.fetchPrompt(ctx, project, name, opts)
	if err == nil {
		c.prompts.put(key, fetched)
		return fetched, nil
	}
	if cached != nil {
		if c.prompts.markReported(key) {
			c.logf("prompt %q could not be fetched (%v); serving the copy fetched %s ago",
				name, err, time.Since(cached.fetched).Round(time.Second))
		}
		return clonePrompt(&cached.prompt), nil
	}
	if fallback, ok := fallbackPrompt(project, name, opts); ok {
		return fallback, nil
	}
	return nil, err
}

// fetchPrompt asks the daemon, with no cache in the way.
func (c *Client) fetchPrompt(ctx context.Context, project, name string, opts PromptOptions) (*Prompt, error) {
	if c == nil {
		return nil, ErrNoClient
	}
	path := "/v1/api/prompts/" + url.PathEscape(name)
	values := c.projectValues(project)
	switch {
	case opts.Version != 0:
		values.Set("version", strconv.FormatUint(uint64(opts.Version), 10))
	case opts.Label != "":
		values.Set("label", opts.Label)
	default:
		values.Set("label", DefaultPromptLabel)
		out := &Prompt{}
		err := c.getJSON(ctx, path, values, out)
		var apiErr *APIError
		if !errors.As(err, &apiErr) || apiErr.Status != http.StatusNotFound {
			if err != nil {
				return nil, err
			}
			return out, nil
		}
		// No version carries the label yet, so the newest one is the
		// answer. Labelling a version later changes what this call returns
		// without a deploy.
		values.Del("label")
	}
	out := &Prompt{}
	if err := c.getJSON(ctx, path, values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// fallbackPrompt builds the prompt the caller supplied for when the daemon
// cannot answer. Version 0 marks it as not stored, which SetPrompt reads.
func fallbackPrompt(project, name string, opts PromptOptions) (*Prompt, bool) {
	p := &Prompt{Project: project, Name: name, Labels: []string{}, Variables: []string{}}
	switch {
	case opts.FallbackTemplate != "":
		template := opts.FallbackTemplate
		p.Kind = PromptKindText
		p.Template = &template
		p.Variables = variablesIn(template, p.Variables)
	case opts.FallbackMessages != nil:
		raw, err := json.Marshal(opts.FallbackMessages)
		if err != nil {
			return nil, false
		}
		p.Kind = PromptKindChat
		p.Messages = raw
		var decoded any
		if decodeJSON(raw, &decoded) == nil {
			p.Variables = variablesInValue(decoded, p.Variables)
		}
	default:
		return nil, false
	}
	return p, true
}

// CreatePrompt stores a new version of a prompt and returns it. A body
// identical to the latest version stores nothing and returns that version, so
// a deploy can post its prompts on every start. It does not clear the read
// cache: a running agent sees the change within its CacheTTL.
func (c *Client) CreatePrompt(ctx context.Context, req CreatePromptRequest) (*Prompt, error) {
	if c == nil {
		return nil, ErrNoClient
	}
	if req.Name == "" {
		return nil, errors.New("mcpd: CreatePrompt needs a prompt name")
	}
	out := &Prompt{}
	if err := c.postJSON(ctx, "/v1/api/prompts", c.projectValues(""), req, out); err != nil {
		return nil, err
	}
	return out, nil
}

// SetPromptLabel points a label at a version. Moving "production" releases a
// version and moving it back rolls one back, with no deploy for either.
func (c *Client) SetPromptLabel(ctx context.Context, name, label string, version uint32) (*Prompt, error) {
	if c == nil {
		return nil, ErrNoClient
	}
	if name == "" || label == "" {
		return nil, errors.New("mcpd: SetPromptLabel needs a prompt name and a label")
	}
	if version == 0 {
		return nil, errors.New("mcpd: SetPromptLabel needs a version; versions start at 1")
	}
	body, err := json.Marshal(PromptLabelRequest{Version: version})
	if err != nil {
		return nil, fmt.Errorf("mcpd: request could not be encoded: %w", err)
	}
	path := "/v1/api/prompts/" + url.PathEscape(name) + "/labels/" + url.PathEscape(label)
	raw, err := c.doRequest(ctx, http.MethodPut, path, c.projectValues(""), body, "", 0)
	if err != nil {
		return nil, err
	}
	out := &Prompt{}
	if err := json.Unmarshal(raw, out); err != nil {
		return nil, decodeError(err)
	}
	return out, nil
}

// --------------------------------------------------------------- linking ---

// SetPrompt links the span to the prompt it used, so ListSpans with Prompt and
// PromptVersion finds it and a trace shows which version produced an answer.
// It sets the attribute mcpd.prompt.name, and mcpd.prompt.version unless the
// version is 0 (a fallback, which was never stored). Other attributes are
// kept. A nil span or prompt, or a prompt with no name, sets nothing.
func (s *ActiveSpan) SetPrompt(p *Prompt) {
	if s == nil || p == nil || p.Name == "" {
		return
	}
	name, version := p.Name, p.Version
	s.mutate(func(d *Span) {
		attrs := map[string]any{}
		if len(d.Attributes) > 0 {
			if err := decodeJSON(d.Attributes, &attrs); err != nil {
				// Attributes that are not an object have no place for a key;
				// overwriting them would lose what the caller set.
				return
			}
			if attrs == nil {
				attrs = map[string]any{}
			}
		}
		attrs[promptNameAttribute] = name
		if version > 0 {
			attrs[promptVersionAttribute] = version
		} else {
			// A version left by an earlier prompt would link this span to
			// a version it did not use.
			delete(attrs, promptVersionAttribute)
		}
		if raw, err := json.Marshal(attrs); err == nil {
			d.Attributes = raw
		}
	})
}

// ------------------------------------------------------------- rendering ---

// RenderedPrompt is a prompt with its variables filled in. A text prompt sets
// Text; a chat prompt sets Messages. Kind says which.
type RenderedPrompt struct {
	// Kind is the prompt's kind.
	Kind PromptKind
	// Text is the rendered template of a text prompt.
	Text string
	// Messages is a copy of a chat prompt's messages in which every string
	// content is rendered. Every other field is copied unchanged.
	Messages []map[string]any
}

// PromptRenderError is returned by RenderPrompt when the prompt names
// variables the caller did not supply.
type PromptRenderError struct {
	// Prompt is the prompt's name.
	Prompt string
	// Missing lists every missing variable in order of first use.
	Missing []string
}

func (e *PromptRenderError) Error() string {
	return fmt.Sprintf("mcpd: prompt %q needs variables that were not given: %s",
		e.Prompt, strings.Join(e.Missing, ", "))
}

// RenderPrompt fills in a prompt's {{variables}} by the daemon's own rule.
//
// A placeholder is "{{", then everything up to the next "}}"; the text
// between, trimmed of whitespace, is the name, so {{city}} and {{ city }} are
// the same. A placeholder whose text is not a name, and a "{{" with no closing
// "}}", is left as written. Substitution is one pass: a value that contains
// {{x}} is not rendered again. Extra variables are ignored, and a missing one
// is a *PromptRenderError, because a prompt with a hole in it is worse than
// no call at all.
func RenderPrompt(p *Prompt, vars map[string]string) (*RenderedPrompt, error) {
	if p == nil {
		return nil, errors.New("mcpd: RenderPrompt needs a prompt")
	}
	kind := p.Kind
	if kind == "" {
		kind = PromptKindText
		if len(p.Messages) > 0 {
			kind = PromptKindChat
		}
	}
	var missing []string
	out := &RenderedPrompt{Kind: kind}
	switch kind {
	case PromptKindText:
		if p.Template == nil {
			return nil, fmt.Errorf("mcpd: text prompt %q has no template", p.Name)
		}
		out.Text = renderTemplate(*p.Template, vars, &missing)
	case PromptKindChat:
		var messages []map[string]any
		if err := decodeJSON(p.Messages, &messages); err != nil {
			return nil, fmt.Errorf("mcpd: chat prompt %q has messages that are not a list of objects: %w", p.Name, err)
		}
		for _, message := range messages {
			if content, ok := message["content"].(string); ok {
				message["content"] = renderTemplate(content, vars, &missing)
			}
		}
		out.Messages = messages
	default:
		return nil, fmt.Errorf("mcpd: prompt %q has kind %q, which cannot be rendered", p.Name, kind)
	}
	if len(missing) > 0 {
		return nil, &PromptRenderError{Prompt: p.Name, Missing: missing}
	}
	return out, nil
}

// renderTemplate substitutes in one pass, adding each missing name to missing
// the first time it is used.
func renderTemplate(template string, vars map[string]string, missing *[]string) string {
	var b strings.Builder
	rest := template
	for {
		open := strings.Index(rest, "{{")
		if open < 0 {
			break
		}
		after := rest[open+2:]
		end := strings.Index(after, "}}")
		if end < 0 {
			break
		}
		b.WriteString(rest[:open])
		name := strings.TrimSpace(after[:end])
		switch value, ok := vars[name]; {
		case !isVariableName(name):
			b.WriteString(rest[open : open+2+end+2])
		case ok:
			b.WriteString(value)
		default:
			if !containsString(*missing, name) {
				*missing = append(*missing, name)
			}
		}
		rest = after[end+2:]
	}
	b.WriteString(rest)
	return b.String()
}

// variablesIn appends every variable a template names, in order of first use,
// without duplicates. It is the daemon's variables_in.
func variablesIn(template string, out []string) []string {
	rest := template
	for {
		open := strings.Index(rest, "{{")
		if open < 0 {
			return out
		}
		after := rest[open+2:]
		end := strings.Index(after, "}}")
		if end < 0 {
			return out
		}
		name := strings.TrimSpace(after[:end])
		if isVariableName(name) && !containsString(out, name) {
			out = append(out, name)
		}
		rest = after[end+2:]
	}
}

// variablesInValue walks every string in a JSON value. Object keys are visited
// in sorted order, as the daemon's JSON maps are, so a fallback lists its
// variables in the order the stored prompt would.
func variablesInValue(v any, out []string) []string {
	switch value := v.(type) {
	case string:
		return variablesIn(value, out)
	case []any:
		for _, item := range value {
			out = variablesInValue(item, out)
		}
	case map[string]any:
		keys := make([]string, 0, len(value))
		for key := range value {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			out = variablesInValue(value[key], out)
		}
	}
	return out
}

// isVariableName is an ASCII letter or '_', then ASCII letters, digits, '_'
// and '.'.
func isVariableName(name string) bool {
	if name == "" {
		return false
	}
	for i := 0; i < len(name); i++ {
		ch := name[i]
		letter := (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || ch == '_'
		if i == 0 && !letter {
			return false
		}
		if !letter && !(ch >= '0' && ch <= '9') && ch != '.' {
			return false
		}
	}
	return true
}

func containsString(list []string, s string) bool {
	for _, item := range list {
		if item == s {
			return true
		}
	}
	return false
}

// decodeJSON keeps numbers as written, so a message or attribute copied
// through it loses no precision.
func decodeJSON(raw []byte, out any) error {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	return dec.Decode(out)
}

// ----------------------------------------------------------------- cache ---

type promptEntry struct {
	prompt  Prompt
	fetched time.Time
	// reported is set once a failed refetch has been logged, so an outage
	// costs one line per prompt rather than one per call.
	reported bool
}

// promptCache is usable at its zero value, so a Client needs no set-up for it.
type promptCache struct {
	mu      sync.Mutex
	entries map[string]*promptEntry
	// order lists keys oldest first, for eviction.
	order []string
}

func promptCacheKey(project, name string, opts PromptOptions) string {
	selector := "default"
	switch {
	case opts.Version != 0:
		selector = "version:" + strconv.FormatUint(uint64(opts.Version), 10)
	case opts.Label != "":
		selector = "label:" + opts.Label
	}
	return project + "\x00" + name + "\x00" + selector
}

// get returns a copy of the entry, so the caller reads it without the lock.
func (pc *promptCache) get(key string) *promptEntry {
	pc.mu.Lock()
	defer pc.mu.Unlock()
	entry, ok := pc.entries[key]
	if !ok {
		return nil
	}
	copied := *entry
	return &copied
}

func (pc *promptCache) put(key string, p *Prompt) {
	pc.mu.Lock()
	defer pc.mu.Unlock()
	if pc.entries == nil {
		pc.entries = make(map[string]*promptEntry)
	}
	if _, ok := pc.entries[key]; ok {
		pc.removeOrder(key)
	} else if len(pc.entries) >= promptCacheSize {
		oldest := pc.order[0]
		pc.order = pc.order[1:]
		delete(pc.entries, oldest)
	}
	pc.entries[key] = &promptEntry{prompt: *clonePrompt(p), fetched: time.Now()}
	pc.order = append(pc.order, key)
}

// markReported reports whether this is the first failure since the entry was
// last fetched.
func (pc *promptCache) markReported(key string) bool {
	pc.mu.Lock()
	defer pc.mu.Unlock()
	entry, ok := pc.entries[key]
	if !ok || entry.reported {
		return false
	}
	entry.reported = true
	return true
}

func (pc *promptCache) removeOrder(key string) {
	for i, k := range pc.order {
		if k == key {
			pc.order = append(pc.order[:i], pc.order[i+1:]...)
			return
		}
	}
}

func (pc *promptCache) len() int {
	pc.mu.Lock()
	defer pc.mu.Unlock()
	return len(pc.entries)
}

// clonePrompt copies every slice and pointer, so a caller that edits the
// prompt it was given cannot change what the cache serves next.
func clonePrompt(p *Prompt) *Prompt {
	out := *p
	out.Template = cloneStringPtr(p.Template)
	out.Note = cloneStringPtr(p.Note)
	out.CreatedBy = cloneStringPtr(p.CreatedBy)
	if p.Messages != nil {
		out.Messages = append(json.RawMessage(nil), p.Messages...)
	}
	if p.Config != nil {
		out.Config = append(json.RawMessage(nil), p.Config...)
	}
	if p.Variables != nil {
		out.Variables = append([]string{}, p.Variables...)
	}
	if p.Labels != nil {
		out.Labels = append([]string{}, p.Labels...)
	}
	return &out
}

func cloneStringPtr(s *string) *string {
	if s == nil {
		return nil
	}
	v := *s
	return &v
}
