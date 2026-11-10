import sys, numpy as np, pandas as pd, collections
df = pd.read_csv(sys.argv[1]); t=df.iloc[:,0].to_numpy()
clk=df.iloc[:,1].to_numpy(); si=df.iloc[:,2].to_numpy(); so=df.iloc[:,3].to_numpy(); bz=df.iloc[:,4].to_numpy()
ce=np.nonzero(np.diff(clk))[0]+1
rise=ce[clk[ce]==1]; fall=ce[clk[ce]==0]
# where do data edges happen relative to clock? check: data transitions closer to falling or rising edge
for name,d in (('TSI',si),('TSO',so)):
    de=np.nonzero(np.diff(d))[0]+1; de=de[:20000]
    tr=t[rise]; tf=t[fall]
    def dist(arr):
        idx=np.searchsorted(arr,t[de]); idx=np.clip(idx,1,len(arr)-1)
        return np.minimum(np.abs(arr[idx]-t[de]),np.abs(arr[idx-1]-t[de]))
    print(name,'median dist to rising %.3fus, to falling %.3fus'%(np.median(dist(tr))*1e6,np.median(dist(tf))*1e6))
# sample on rising edges (value = row before? use row index of edge: state at that row is after edge; data stable)
samp_si=si[rise]; samp_so=so[rise]; tr=t[rise]
d=np.diff(tr); gaps=np.nonzero(d>20e-6)[0]; starts=np.r_[0,gaps+1]; ends=np.r_[gaps+1,len(tr)]
def tobytes(bits,lsb):
    bits=bits[:len(bits)//8*8].reshape(-1,8)
    w = (1<<np.arange(8)) if lsb else (1<<np.arange(7,-1,-1))
    return bytes((bits*w).sum(1).astype(np.uint8))
frames=[]
for s,e in zip(starts,ends):
    frames.append((tr[s], samp_si[s:e], samp_so[s:e]))
for lsb in (True,False):
    print('\n=== bit order', 'LSB-first' if lsb else 'MSB-first')
    for nm,k in (('TSI (main->panel)',1),('TSO (panel->main)',2)):
        c=collections.Counter(tobytes(f[k],lsb).hex(' ') for f in frames)
        print(nm,'unique frames:',len(c))
        for fr,n in c.most_common(6): print('  %6d  %s'%(n,fr))
np.save(sys.argv[2], np.array([(f[0],tobytes(f[1],True),tobytes(f[2],True)) for f in frames],dtype=object), allow_pickle=True)
