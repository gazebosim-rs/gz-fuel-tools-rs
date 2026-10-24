# Fuel metadata schema

The four Apache-2.0 schema files in `gz/msgs/` are retained unchanged from
Gazebo gz-msgs commit `89c0c3d1474f545cb889335daf930bfdab6ab852`, the revision
pinned in `scripts/ci/dependencies.lock`. Their original copyright notices remain.

`descriptor.pb` is their checked-in descriptor set, including imported schemas.
Production builds embed it and need no `protoc` executable or Gazebo libraries.
`prost-reflect` supplies schema-aware protobuf text parsing; the matching typed
Rust messages in `src/metadata.rs` carry domain data without JSON intermediates.

Regenerate the descriptor from the repository root with:

```sh
protoc -I src/metadata --include_imports \
  --descriptor_set_out=src/metadata/descriptor.pb \
  src/metadata/gz/msgs/fuel_metadata.proto
```

`python3 scripts/check_metadata_schema.py` verifies reproducibility and a corpus
of accepted/rejected protobuf text. Passing `--binary /path/to/gz-fuel` additionally
compares the Rust CLI acceptance and decoded metadata against upstream protobuf.
The regression suite requires `protoc`; the ordinary Rust build does not.
