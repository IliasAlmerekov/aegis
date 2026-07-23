literal_target=/tmp/literal
rm "$literal_target"
printf '%s\n' replacement > "$target"
rm "$(rm /tmp/inside-substitution)"
source "$script_path"
bash -c "$payload"
python3 -c "$payload"
