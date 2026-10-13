extends Node

signal finished
@export var enabled: bool = true
const LIMIT: int = 3

## Starts the standalone fixture.
func _ready() -> void:
	get_tree().quit()

func _on_quit_pressed() -> void:
	get_tree().quit()
