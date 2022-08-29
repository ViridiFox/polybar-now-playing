default:
	@{{just_executable()}} -f {{justfile()}} --list

install:
	cargo build --release --target-dir target
	mv target/release/now_playing ../../now-playing-rs
