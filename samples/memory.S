# Loads and stores, including the offset-less form
lb    t0, 0(a0)                          # 00050283
lh    t1, 2(a0)                          # 00251303
lw    t2, -4(sp)                         # ffc12383
lbu   a1, 2047(a2)                       # 7ff64583
lhu   a3, -2048(a4)                      # 80075683
lw    a5, (a6)                           # 00082783
sb    t0, 0(a0)                          # 00550023
sh    t1, -2(a0)                         # fe651f23
sw    ra, 12(sp)                         # 00112623
sw    s0, (sp)                           # 00812023
