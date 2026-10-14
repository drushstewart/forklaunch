# Blueprint core

`BlueprintError` carries an optional opaque typed diagnostic context through
`with_diagnostic_context` and `diagnostic_context`. Cloning retains that context;
Debug, Display, equality and public `details` exclude it. Transports continue
selecting their existing wire fields explicitly. A private marker type can
preserve internal error provenance without trusting caller-supplied JSON details.

This is a source compatibility change for external struct literals or exhaustive
struct destructuring. Construct errors with `BlueprintError::new(code, message)`
and `.with_detail(...)`; read the existing public fields or use a pattern with
`..` rather than naming every field. Repository callers already use constructors.
The opaque context is not a serialization or public diagnostic payload API.
