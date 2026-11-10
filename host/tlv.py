def parse(frames):
    """frames: dict type->bytes for 0x81,0x82,0x83... ; returns list of (id, attrs, text)"""
    buf=b''.join(bytes(frames[k][1:15]) for k in sorted(frames) if 0x81<=k<=0x8f)
    out=[]; i=0
    while i+5<=len(buf):
        rid=buf[i]; attrs=buf[i+1:i+4]; n=buf[i+4]
        if rid==0: break
        txt=buf[i+5:i+5+n]; out.append((rid,attrs.hex(),txt.decode('latin1'))); i+=5+n
    return out
