printf '%s\n' replacement > data.txt
printf '%s\n' extra >> data.txt
tee output.txt < input.txt
tee --append output.txt < input.txt
