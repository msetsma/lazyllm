.PHONY: demo clean-db build run

DB_PATH := $(HOME)/.local/share/lazyllm/lazyllm.db

build:
	cargo build --release

clean-db:
	sqlite3 $(DB_PATH) "DELETE FROM conversations; DELETE FROM messages;"

run: build
	./target/release/lazyllm

demo: build clean-db
	vhs demo/demo.tape
