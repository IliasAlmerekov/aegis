# rm -rf / is a comment, not an invocation.
echo 'rm -rf /tmp/not-executed'
rm_helper() { echo harmless; }
rm_helper /tmp/not-executed
[[ -f data.txt ]]
export ACTION=rm
unset ACTION
