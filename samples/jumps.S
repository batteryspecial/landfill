# Jumps, upper immediates and system instructions
main:
    jal   ra, func                       # 008000ef
    jal   zero, end                      # 01c0006f
func:
    lui   t0, 0x12345                    # 123452b7
    auipc t1, 0xFFFFF                    # fffff317
    lui   t2, 0                          # 000003b7
    jalr  zero, 0(ra)                    # 00008067
    jalr  ra, t0, -4                     # ffc280e7
    jalr  x1, (t0)                       # 000280e7
end:
    ebreak                               # 00100073
    ecall                                # 00000073
    jal   x0, 0                          # 0000006f
