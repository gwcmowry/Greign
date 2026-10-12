# Greign XMP sidecar contract

Greign owns the namespace:

```text
https://greign.org/ns/1.0/
```

The owned RDF description is delimited by `<!-- greign:begin -->` and
`<!-- greign:end -->`. Greign replaces only this block. Elements, attributes,
comments, and namespace declarations outside it are retained byte-for-byte.

`xmp:Rating`, `xmp:Label`, and `dc:subject` expose standard metadata. The
`greign:Payload` attribute contains XML-escaped canonical JSON for the base
recipe, flag, standard metadata mirror, and virtual copies. The JSON mirror is
the authoritative lossless reconstruction payload for schema 1.

Writes use same-directory temporary files, file and directory `fsync`, and
atomic rename. A caller that previously read a sidecar should supply its
SHA-256 snapshot; a mismatch reports a conflict and does not overwrite the
newer file.
