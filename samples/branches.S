# Every branch, to labels in both directions and to raw offsets
start:
    beq   a0, a1, done                   # 02b50063
    bne   a0, zero, start                # fe051ee3
    blt   t0, t1, start                  # fe62cce3
loop: bge t0, t1, done                   # 0062da63
    bltu  a2, a3, loop                   # fed66ee3
    bgeu  a2, a3, loop                   # fed67ce3
    beq   x0, x0, 8                      # 00000463
    bne   x0, x0, -4                     # fe001ee3
done:
    ecall                                # 00000073
