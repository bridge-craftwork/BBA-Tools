"""Concurrency test against a running bba-server, i.e. through the native library in epbot-libs/.

Bids every deal once sequentially, then 3 shuffled rounds on 16 client threads, and reports
auctions that differ. Start the server with MAX_CONCURRENCY=8 (or more) so auctions overlap.

usage: python3 server_concurrency.py <base-url> <pbn>...
   eg: python3 server_concurrency.py http://127.0.0.1:5099 ../../tests/fixtures/slow/*.pbn
"""
import sys,re,json,random,urllib.request
from concurrent.futures import ThreadPoolExecutor
deals=[]
URL=sys.argv[1].rstrip('/')+'/api/auction/generate'
for f in sys.argv[2:]:
    d,v="N","None"
    for l in open(f):
        m=re.match(r'\[(\w+) "(.*)"\]',l)
        if not m: continue
        k,val=m.groups()
        if k=="Dealer": d=val
        elif k=="Vulnerable": v=val
        elif k=="Deal": deals.append({"pbn":val,"dealer":d,"vulnerability":v})
def bid(i):
    r=urllib.request.Request(URL,json.dumps({"deal":deals[i]}).encode(),{"Content-Type":"application/json"})
    j=json.load(urllib.request.urlopen(r,timeout=60)); return " ".join(j.get("auction") or ["FAIL",str(j.get("error"))])
ref=[bid(i) for i in range(len(deals))]
print(len(deals),"deals, sequential failures:",sum(r.startswith("FAIL") for r in ref))
tot=bad=0
for rnd in range(3):
    order=list(range(len(deals))); random.shuffle(order)
    with ThreadPoolExecutor(16) as ex:
        for i,got in zip(order,ex.map(bid,order)):
            tot+=1
            if got!=ref[i]:
                bad+=1
                if bad<=3: print(" deal",i,"seq",ref[i],"| par",got)
print(f"concurrent: {tot} auctions, {bad} differ from sequential")
