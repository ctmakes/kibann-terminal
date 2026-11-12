#!/bin/sh
# Draws frames inside synchronized output (mode 2026). Between the start and
# end of each hold the screen is cleared and half drawn, which a terminal must
# never show.
printf '\033[?1049h\033[?25l'
i=1
while [ $i -le 6 ]; do
  printf '\033[?2026h\033[2J\033[H'
  printf '\033[1;36m┌──────────────────────────┐\033[0m\r\n'
  sleep 0.05
  printf '\033[1;36m│\033[0m  frame %d of 6            \033[1;36m│\033[0m\r\n' $i
  printf '\033[1;36m│\033[0m  \033[42m%*s\033[0m%*s\033[1;36m│\033[0m\r\n' $((i*4)) '' $((24-i*4)) ''
  printf '\033[1;36m└──────────────────────────┘\033[0m\r\n'
  printf '\033[?2026l'
  sleep 0.2
  i=$((i+1))
done
sleep 5
