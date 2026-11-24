"""Link Tonfa after the executable mods present in the target game layout."""
from pathlib import Path
import argparse, hashlib, json, struct, subprocess, sys

U=struct.Struct('<I')
MAIN=0x200000
FIRST_SUBSDK=0x2915000
SITES={0x13c208:'base',0x13eef8:'base',0x13ed08:'level',0x6f9658:'create',
       0x70153c:'process',0xb83c48:'text',0x286ac0:'model_file',0x152960:'file_resource',
       0x560920:'icon_rect'}
LITERALS=((0x6f2da8,0x6f2cec,'tonfa_order',0),(0x6f2b1c,0x6f2a94,'tonfa_labels',0),
          (0x6f29ac,0x6f1d2c,'tonfa_labels',0),(0x5933f8,0x5930b8,'tonfa_shop_icons',0),
          (0xdae78,0xdae6c,'tonfa_equip_slots',0),(0xdb668,0xdb5f8,'tonfa_equip_slots',0),
          (0xdb7b0,0xdb6a0,'tonfa_equip_slots',0),
          (0xdaeb4,0xdaea8,'tonfa_actor_classes',0))
ICON_TABLE_USES=(
    ('tonfa_common_icons',((0x560c6c,0x560c60),(0x560ca0,0x560c94))),
    ('tonfa_box_icons',((0x6eb9e4,0x6eb714),)),
    ('tonfa_forge_icons',((0x6f89f0,0x6f875c),(0x6fe434,0x6fe3d4),
                         (0x7014e4,0x7005fc),(0x701810,0x701798),(0x702d80,0x7029e4),
                         (0x70b3e0,0x70aed8))),
)
ROSTER=(0x634268,)
PREVIEW_TYPE = 0x4afc24
HIT_CALL = 0x16c938
# GU keeps the saved identity in the gear entry. These comparisons only
# classify the item before the native reader looks up that entry again.
# Treat type 25 as a single-element melee weapon for those checks.
WEAPON_CHECKS = {
    0xdaef0: ('r0', 'sub r1,r0,#7'),   # IsWeapon(entry)
    0xdaf04: ('r0', 'sub r1,r0,#7'),   # IsWeapon(type)
    0xdb7f4: ('r0', 'sub r0,r0,#7'),   # attack
    0xdb8a0: ('r0', 'sub r0,r0,#7'),   # affinity
    0xdb914: ('r0', 'cmp r0,#21'),     # sharpness
    0xdb99c: ('r0', 'cmp r0,#22'),     # element
    0xdba70: ('r0', 'cmp r0,#22'),     # element amount
    0xdbbac: ('r0', 'cmp r0,#22'),     # equipment property
    0x2874cc: ('r0', 'sub r0,r0,#7'), # hunter weapon model
    0x57f1b0: ('r0', 'sub r1,r0,#7'), # add melee stats page to Equipment Details
    0x4addd4: ('r0', 'add r0,r0,#0xf9'), # class of the smithy preview equipment
    0x4af994: ('r0', 'add r0,r0,#0xf9'), # class when rebuilding the preview
    0x26ee78: ('r0', 'sub r0,r0,#7'), # paired-weapon preview configuration
    0x4add38: ('r1', 'cmp r1,#22'),   # select weapon viewer instead of the empty pane
    0x4adf5c: ('r1', 'cmp r1,#21'),   # native viewer controls for this category
    0x4adc54: ('r0', 'add r0,r1,r0,lsl #6'), # left/right preview transform table
    0x4adca0: ('r0', 'sub r0,r0,#8'), # second visible model in the paired preview
}
def load_ips(data):
    if data[:5]!=b'IPS32':raise ValueError('Expected IPS32')
    patches={};i=5
    while data[i:i+4]!=b'EEOF':
        o,n=struct.unpack_from('>IH',data,i);i+=6
        if not n:raise ValueError('Unexpected RLE record')
        patches[o-0x100]=data[i:i+n];i+=n
    return patches
def symbol_map(b):
    sh=struct.unpack_from('<I',b,32)[0];size,n=struct.unpack_from('<HH',b,46)
    rows=[struct.unpack_from('<10I',b,sh+i*size) for i in range(n)]
    sym=next(r for r in rows if r[1]==2);strings=rows[sym[6]][4];out={}
    for i in range(sym[4],sym[4]+sym[5],sym[9]):
        name,v=struct.unpack_from('<2I',b,i);p=strings+name
        out[b[p:b.index(0,p)].decode()]=v
    return out
def target(site,opcode):
    if opcode&0xfffff000!=0xe28ff000:raise ValueError('Unknown previous entry')
    n=((opcode>>8)&15)*2;imm=opcode&255
    return MAIN+site+8+(((imm>>n)|(imm<<(32-n)))&0xffffffff)
def jump(addr,target):
    d=target-addr-8
    if d%4 or not -0x2000000<=d<0x2000000:raise ValueError('Unreachable branch')
    return 0xea000000|((d//4)&0xffffff)
def build(reference,game,zig,temp,dest):
    port=reference/'porting';up=next((port/'third_party').glob('MHGU-Definitive*'));mod=up/'module'
    sys.path[:0]=[str(port),str(up/'tools')]
    from hook_codegen import restore
    import nso
    temp.mkdir(parents=True,exist_ok=True);dest.mkdir(parents=True,exist_ok=True)
    src=Path(__file__).resolve().parent
    mods=game/'mods'
    BASE=FIRST_SUBSDK
    previous={}
    expected={1:{'022ca7851796022ef36251da300794d7f05fae6bfe0f1a434409995ee6f6e561',
                 '312f514bfb681f6269010e8594d4ab6a93d1f8a9ccb91d9c97ac203ca9b45e49'},
              2:{'67d6e075284aec676d1478228b1b4414c5b47268ff8cd181a4b448657f2b55f8'}}
    ip_name='FB08F1D20FD1204FE7070672777F869B.ips'
    with_magnet=False
    for slot in (1,2):
        candidates=list(mods.glob(f'*/exefs/subsdk{slot}'))
        if not candidates:continue
        if len(candidates)!=1:raise ValueError(f'Multiple providers of subsdk{slot}')
        path=candidates[0];image=path.read_bytes()
        if hashlib.sha256(image).hexdigest() not in expected[slot]:
            raise ValueError(f'Unknown executable layout: {path}')
        length=(sum(struct.unpack_from('<3I',image,0x34))+4095)&~4095
        for site,raw in load_ips((path.parent/ip_name).read_bytes()).items():
            if site in SITES:
                entry=target(site,U.unpack(raw)[0])
                if not BASE<=entry<BASE+length:raise ValueError(f'Bad hook at {site:x}')
                previous[site]=entry
        BASE+=length
        with_magnet |= slot==2
    policy=json.loads((port/'balance-policy.json').read_text())
    # Original first instructions for entry points that have no prior hook.
    stock={0x13c208:'push {r4,r5,r6,lr}',0x13eef8:'push {fp,lr}',0x13ed08:'push {fp,lr}',
           0x6f9658:'push {fp,lr}',0x70153c:'push {fp,lr}',0xb83c48:'mov r0,r8',
           0x286ac0:'push {r4,r5,r6,r7,fp,lr}',0x152960:'movw r2,#0x6cf',
           0x560920:'push {r4,r5,r6,lr}'}
    text=[]
    for site,fn in SITES.items():
        text.extend([f'hook_{site:x}:','push {r0-r5,r12,lr}','mrs r12,apsr','vmrs r0,fpscr',
            'push {r0,r12}','vpush {d0-d15}','vpush {d16-d31}','add r0,sp,#264',
            f'bl {fn}_c','cmp r0,#0',f'bne handled_{site:x}',*restore(),
            f'chain_{site:x}:'])
        if site in previous:
            text.append('.word 0')
        else:
            text.extend([stock[site],'ldr ip,[pc,#0]','ldr pc,[pc,ip]',
                f'.word native_{site:x} - (. + 4)', '.pushsection .data','.align 2',
                f'native_{site:x}: .word __module_start - MAIN_GAP + {site+4}', '.popsection'])
        text.extend([f'handled_{site:x}:',*restore(),'bx lr'])
    for site in ROSTER:
        text.extend([f'hook_{site:x}:','push {r1-r5,lr}'])
        text.extend(['bl roster_icon_c','pop {r1-r5,lr}'])
        text.extend(['ldr ip,[pc,#0]', 'ldr pc,[pc,ip]', f'.word ret_{site:x} - (. + 4)',
                     '.pushsection .data', '.align 2', f'ret_{site:x}:',
                     f'.word __module_start - MAIN_GAP + {site+4}', '.popsection'])
    for site,(reg,displaced) in WEAPON_CHECKS.items():
        # Save NZCV for a displaced SUB without S. Conditional users of
        # the displaced CMP receive the flags from that comparison.
        native = 18 if site in (0x4addd4, 0x4af994, 0x26ee78, 0x4add38, 0x4adf5c, 0x4adc54, 0x4adca0) else 7
        text.extend([f'hook_{site:x}:','push {ip,lr}','mrs ip,apsr',
                     f'cmp {reg},#25',f'moveq {reg},#{native}'])
        if not displaced.startswith('cmp'):
            text.append('msr apsr_nzcvq,ip')
        text.extend([displaced,'ldr ip,[pc,#8]','ldr ip,[pc,ip]',
                     'str ip,[sp,#4]','pop {ip,pc}',
                     f'.word ret_{site:x} - (. - 4)',
                     '.pushsection .data','.align 2',f'ret_{site:x}:',
                     f'.word __module_start - MAIN_GAP + {site+4}', '.popsection'])
    # Keep the public damage dispatcher free for damage-number mods. The
    # private observer wraps only GU's call from the monster collision pass.
    text.extend([f'hook_{HIT_CALL:x}:','ldr lr,[pc,#4]','ldr lr,[pc,lr]',
                 'b hit_observer','.word hit_return - (. + 0)',
                 'hit_observer:', 'nop','nop','ldr ip,[pc,#0]','ldr pc,[pc,ip]',
                 '.word hit_dispatch - (. + 4)', '.pushsection .data','.align 2',
                 f'hit_return: .word __module_start - MAIN_GAP + {HIT_CALL+4}',
                 'hit_dispatch: .word __module_start - MAIN_GAP + 0x170968','.popsection'])
    text.extend([f'hook_{PREVIEW_TYPE:x}:','push {ip,lr}','mrs ip,apsr','cmp r1,#25',
                 'moveq r8,#11','subne r8,r1,#7','msr apsr_nzcvq,ip',
                 'ldr ip,[pc,#8]','ldr ip,[pc,ip]','str ip,[sp,#4]','pop {ip,pc}',
                 '.word preview_return - (. - 4)', '.pushsection .data','.align 2',
                 f'preview_return: .word __module_start - MAIN_GAP + {PREVIEW_TYPE+4}', '.popsection'])
    text.extend(['.section .hookwin,"ax"',' .align 2','window: .space 0x40000'])
    (temp/'hooks.inc').write_text('\n'.join(text)+'\n')
    (temp/'layout.inc').write_text(f'.equ MAIN_GAP,{BASE-MAIN}\n')
    flags=['-target','arm-linux-gnueabihf','-mcpu=cortex_a57','-marm','-fPIC','-fvisibility=hidden',
           '-ffreestanding','-fno-builtin','-fno-stack-protector','-nostdlib','-O2',
           '-fno-unwind-tables','-fno-asynchronous-unwind-tables','-I',str(mod),'-I',str(temp),
           f'-DWITH_MAGNET={int(with_magnet)}',f'-DRAW_PERCENT={int(policy["raw_percent"])}',
           f'-DELEMENT_PERCENT={int(policy["element_percent"])}']
    objects=[]
    # Zig's assembler cache does not track GAS .include inputs. Expand them
    # into the translation unit so a changed hook list always recompiles.
    startup=(mod/'crt0.S').read_text()
    for include in ('layout.inc','hooks.inc'):
        startup=startup.replace(f'.include "{include}"',(temp/include).read_text())
    (temp/'crt0.S').write_text(startup)
    for f in (temp/'crt0.S',mod/'util.c',src/'equipment.c'):
        obj=temp/(f.stem+'.o')
        subprocess.run([str(zig),'cc',*flags,'-c',str(f),'-o',str(obj)],check=True)
        objects.append(str(obj))
    elf=temp/'tonfa.elf'
    subprocess.run([str(zig),'ld.lld','-m','armelf_linux_eabi','-shared','-Bsymbolic',
          '--hash-style=sysv','-T',str(mod/'link.ld'),'-z','max-page-size=4096',
          '-init=module_init','--no-undefined','-o',str(elf),*objects],check=True)
    blob=elf.read_bytes();syms=symbol_map(blob);out=bytearray(nso.elf2nso(blob))
    foff=U.unpack_from(out,0x10)[0]
    patches={}
    for site in (*SITES,*ROSTER,*WEAPON_CHECKS,PREVIEW_TYPE,HIT_CALL):
        if site in previous:
            chain=syms[f'chain_{site:x}']
            U.pack_into(out,foff+chain,jump(BASE+chain,previous[site]))
        start=syms['window']
        off=start+((MAIN+site+8-BASE-start)%0x40000)
        assert U.unpack_from(out,foff+off)[0]==0
        U.pack_into(out,foff+off,jump(off,syms[f'hook_{site:x}']))
        d=BASE+off-(MAIN+site+8); assert d%0x40000==0 and 0<d//0x40000<256
        patches[site]=U.pack(0xe28ff700|d//0x40000)
    for site,use,label,_ in LITERALS:
        # Preserve the source's small pointer displacement, e.g. jump-table
        # literals addressing the same label array from different instructions.
        patches[site]=U.pack((BASE+syms[label]-(MAIN+use+8))&0xffffffff)
    for label, uses in ICON_TABLE_USES:
        for site,use in uses:
            patches[site]=U.pack((BASE+syms[label]-(MAIN+use+8))&0xffffffff)
    patches[0x1650080]=U.pack(0x0509000f+int(with_magnet))
    # The two entry-to-base resolvers accept consecutive equipment types
    # 1..24. Type 25 has its own bounded base_c lookup.
    patches[0x13f048]=U.pack(0xe3520018)
    patches[0x13f07c]=U.pack(0xe3510018)
    for i in range(3):
        f,_,n=struct.unpack_from('<3I',out,16+16*i);out[160+32*i:192+32*i]=hashlib.sha256(out[f:f+n]).digest()
    (dest/'subsdk3').write_bytes(out)
    (dest/ip_name).write_bytes(b'IPS32'+b''.join(struct.pack('>IH',o+0x100,len(raw))+raw for o,raw in sorted(patches.items()))+b'EEOF')
    print(f'Tonfa linked for {BASE:08x}, Magnet Spike {"present" if with_magnet else "absent"}')
if __name__=='__main__':
    p=argparse.ArgumentParser()
    for arg in ('reference','game','zig','temp','dest'):p.add_argument(arg,type=lambda v:Path(v).resolve())
    build(**vars(p.parse_args()))
