import random, sys, json
from multiprocessing import Pool
ALL=0x1FF
PEERS=[]
for i in range(81):
    r,c=divmod(i,9);s=set()
    for k in range(9):
        s.add(r*9+k);s.add(k*9+c)
    br,bc=r//3*3,c//3*3
    for a in range(3):
        for b in range(3):s.add((br+a)*9+bc+b)
    s.discard(i);PEERS.append(sorted(s))
UNITS=[[r*9+c for c in range(9)] for r in range(9)]+[[r*9+c for r in range(9)] for c in range(9)]+[[(br+a)*9+bc+b for a in range(3) for b in range(3)] for br in (0,3,6) for bc in (0,3,6)]

def cands(g):
    cs=[0]*81
    for i in range(81):
        if g[i]==0:
            m=0
            for p in PEERS[i]:
                if g[p]:m|=1<<(g[p]-1)
            cs[i]=ALL&~m
    return cs

def count(g,limit=2):
    g=g[:]
    n=[0]
    def rec():
        if n[0]>=limit:return
        cs=cands(g);best=-1;bc=10
        for i in range(81):
            if g[i]==0:
                c=bin(cs[i]).count("1")
                if c==0:return
                if c<bc:bc=c;best=i
                if c==1:break
        if best<0:n[0]+=1;return
        m=cs[best]
        while m:
            b=m&-m;m^=b
            g[best]=b.bit_length();rec();g[best]=0
            if n[0]>=limit:return
    rec();return n[0]

def solve(g):
    g=g[:]
    def rec():
        cs=cands(g);best=-1;bc=10
        for i in range(81):
            if g[i]==0:
                c=bin(cs[i]).count("1")
                if c==0:return False
                if c<bc:bc=c;best=i
        if best<0:return True
        m=cs[best]
        opts=[b.bit_length() for b in [1<<k for k in range(9)] if m&b]
        random.shuffle(opts)
        for v in opts:
            g[best]=v
            if rec():return True
        g[best]=0;return False
    rec();return g

def full():
    g=[0]*81
    # seed first row random for variety
    row=list(range(1,10));random.shuffle(row);g[:9]=row
    return solve(g)

def grade(p):
    """returns (level_class, guesses). Singles propagation + DFS counting branch decisions."""
    g=p[:]
    hidden_needed=False
    def propagate(g):
        nonlocal hidden_needed
        hn=False
        while True:
            cs=cands(g);ch=False
            for i in range(81):
                if g[i]==0:
                    if cs[i]==0:return None,hn
                    if cs[i]&(cs[i]-1)==0:
                        g[i]=cs[i].bit_length();ch=True;cs=cands(g)
            if ch:continue
            for u in UNITS:
                for d in range(9):
                    pos=[i for i in u if g[i]==0 and cs[i]>>d&1]
                    if len(pos)==1 and all(g[i]!=d+1 for i in u):
                        g[pos[0]]=d+1;ch=True;hn=True;break
                if ch:break
            if not ch:return g,hn
    g2,hn=propagate(g)
    if g2 is not None and 0 not in g2:
        return ("hidden" if hn else "naked"),0
    # count branches
    br=[0]
    def rec(g):
        g,_=propagate(g)
        if g is None:return False
        cs=cands(g);best=-1;bc=10
        for i in range(81):
            if g[i]==0:
                c=bin(cs[i]).count("1")
                if c<bc:bc=c;best=i
        if best<0:return True
        m=cs[best]
        for k in range(9):
            if m>>k&1:
                br[0]+=1
                h=g[:];h[best]=k+1
                if rec(h):return True
        return False
    rec(g)
    return "guess",br[0]

def make(seed):
    random.seed(seed)
    want=seed%4  # 0 easy 1 medium 2 hard 3 expert
    for _ in range(200):
        sol=full();p=sol[:]
        target={0:random.randint(38,42),1:random.randint(32,36),2:random.randint(28,31),3:random.randint(23,27)}[want]
        order=list(range(81));random.shuffle(order)
        clues=81
        for i in order:
            if clues<=target:break
            j=80-i
            if p[i]==0:continue
            a,b=p[i],p[j]
            p[i]=0
            if j!=i and p[j]:p[j]=0
            if count(p)==1:
                clues=sum(1 for x in p if x)
            else:
                p[i]=a;p[j]=b
        clues=sum(1 for x in p if x)
        if abs(clues-target)>2:continue
        cl,gs=grade(p)
        ok={0:cl=="naked",1:cl=="hidden",2:cl=="guess" and gs<=6 or cl=="hidden" and clues<=31,3:cl=="guess" and gs>=7}[want]
        if want==2 and cl=="guess" and gs<=6:ok=True
        if ok:return want,"".join(map(str,p)),"".join(map(str,sol)),clues,cl,gs
    return want,None,None,0,"fail",0

if __name__=="__main__":
    import time
    n=int(sys.argv[1]);t=time.time()
    with Pool() as pool:
        res=pool.map(make,range(1000,1000+n),chunksize=4)
    out={0:[],1:[],2:[],3:[]};stats={}
    seen=set()
    for w,p,s,c,cl,gs in res:
        if p and p not in seen:
            seen.add(p);out[w].append(p)
            stats.setdefault(w,[]).append((c,cl,gs))
    json.dump(out,open("puz.json","w"))
    print({k:len(v) for k,v in out.items()},round(time.time()-t,1),"s")
    for w,v in stats.items():print(w,min(x[0] for x in v),max(x[0] for x in v),sorted(set(x[1] for x in v)),max(x[2] for x in v))
