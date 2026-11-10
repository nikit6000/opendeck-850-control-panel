import sys, numpy as np
fr=np.load(sys.argv[1],allow_pickle=True)
# panel->main: mask known noisy fields
mask={1:{5,6,7,8,9,10,11,12,15}, 2:{8,11,12,13,14,15}}
LOW={(1,13):0x0F}  # jog flags live in high nibble
print('PANEL -> MAIN changes:')
prev={}
for f in fr:
    b=f[1]
    if len(b)<16 or not any(b) or b[0] not in mask: continue
    key=tuple(b[i]&LOW.get((b[0],i),0xFF) for i in range(1,16) if i not in mask[b[0]])
    if b[0] in prev and prev[b[0]]!=key:
        print('  %6.3fs type%02x  %s'%(f[0],b[0],' '.join(('%02x'%b[i]) if i not in mask[b[0]] else '..' for i in range(16))))
    prev[b[0]]=key
print('MAIN -> PANEL changes (excluding heartbeat 0x60):')
prev={}
for f in fr:
    b=f[2]
    if len(b)<16 or not any(b) or b[0]==0x60: continue
    k=bytes(b[:15])
    if b[0] in prev and prev[b[0]]!=k:
        print('  %6.3fs  %s  |%s|'%(f[0],b.hex(' '),''.join(chr(c) if 32<=c<127 else '.' for c in b)))
    prev[b[0]]=k
