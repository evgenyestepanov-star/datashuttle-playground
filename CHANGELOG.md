# Changelog

## [0.2.0](https://github.com/datashuttle-io/playground/compare/datashuttle-playground-v0.1.0...datashuttle-playground-v0.2.0) (2026-05-15)


### Features

* file-s3-mixed-formats + file-bad-encoding cloud-eligible ([060f8f9](https://github.com/datashuttle-io/playground/commit/060f8f962a22ec907d0af95b26a935b9ef03d5de))
* **playground:** postgres-cdc-fanout scenario (6 steps, 5 branches) ([0eab53d](https://github.com/datashuttle-io/playground/commit/0eab53d5c0c4014e2e0d57b7afd841a805ffe9c3))
* **playground:** redis-streams-cdc scenario — continuous XREADGROUP demo ([0dc2d33](https://github.com/datashuttle-io/playground/commit/0dc2d3332f52b2b0c9018a333a86e4ea5c8a3bc3))
* **playground:** redis-streams-events scenario + dispatcher branch ([8270267](https://github.com/datashuttle-io/playground/commit/82702675190acbe23d196b27a72c9f2c292e3804))
* TTL session reaper + S3 purge in playground teardown ([51844d0](https://github.com/datashuttle-io/playground/commit/51844d096810bf644b1334b2493def2554050fe4))


### Bug Fixes

* **ci:** switch release-please to simple + Cargo.toml marker ([60f768a](https://github.com/datashuttle-io/playground/commit/60f768a3262b1d287b68a8bf08a0a05a4715bd0b))
* file-ingestion shuttle SQL — TYPE FILE, real minio endpoint, per-session prefix ([f5477a0](https://github.com/datashuttle-io/playground/commit/f5477a0e3714c93ebcb15f5cd55a3851bd353451))
* mysql playground SQL — drop server_id string, leaner init.sql without GRANT ([506b34c](https://github.com/datashuttle-io/playground/commit/506b34c17d74d005f9c3209cc5c06eee2a9e8354))
* persist playground sessions + sweep api orphans on boot ([9c943cf](https://github.com/datashuttle-io/playground/commit/9c943cf2959daeecf3ccfe9a238ec484e00879aa))
* produce_kafka pre-creates topic via rpk, chunks at 500 records ([4c48b98](https://github.com/datashuttle-io/playground/commit/4c48b98a33258bf3e86a6335036d1ae9bd126f73))
* produce_kafka splits REST port (8082) from native broker port (9092) ([ac87dbf](https://github.com/datashuttle-io/playground/commit/ac87dbf5bad453c013dbdccee3043077fcbd3264))
* produce_kafka treats TOPIC_ALREADY_EXISTS on rpk stdout as success ([2138e96](https://github.com/datashuttle-io/playground/commit/2138e96a8307034a27b04ccb9fe86ab56ee1e13f))
* produce_kafka uses Pandaproxy REST instead of docker-compose-exec ([ee430dc](https://github.com/datashuttle-io/playground/commit/ee430dc4b1c743d24e18ffb28def685426de33b5))
* upload_file uses mc directly against MinIO (no docker-compose-exec) ([199ca45](https://github.com/datashuttle-io/playground/commit/199ca4510f42376f755ec4a46d158677ea2c7a53))
