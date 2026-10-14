# Sum the 4 words at 0x10000000 into a0.

        addi  sp, sp, -16                # ff010113
        sw    ra, 12(sp)                 # 00112623
        lui   a0, 0x10000                # 10000537
        addi  a1, zero, 4                # 00400593
        addi  t0, zero, 0                # 00000293

loop:   beq   a1, zero, done             # 00058c63  stop when the count hits 0
        lw    t1, 0(a0)                  # 00052303
        add   t0, t0, t1                 # 006282b3
        addi  a0, a0, 4                  # 00450513
        addi  a1, a1, -1                 # fff58593
        jal   zero, loop                 # fedff06f

done:   addi  a0, t0, 0                  # 00028513
        lw    ra, 12(sp)                 # 00c12083
        addi  sp, sp, 16                 # 01010113
        jalr  zero, 0(ra)                # 00008067
