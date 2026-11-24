/* GPL-3.0-or-later. GU 1.4.0 local equipment service. */
#include "game.h"
#include "util.h"

enum { TONFA = 25, GS = 7, FAMILIES = 137, DIVINE = 137, MODEL_FIRST = 501 };
typedef unsigned U;
typedef unsigned char Byte;
typedef Byte *(*Getter)(void *, U, U);
static U rows[FAMILIES+1][10], base_vt[92];
static Byte *donor[FAMILIES+1];
static const char *cached_names[FAMILIES+1][3];
static Byte cached_language[FAMILIES+1][3];
static char names[FAMILIES+1][3][72];

#if WITH_MAGNET
#define OPTIONAL_SPIKE(x) x,
#define TONFA_LABEL 207
#else
#define OPTIONAL_SPIKE(x)
#define TONFA_LABEL 206
#endif
const Byte tonfa_order[]={7,14,8,18,9,19,10,16,15,21,20,13,11,17,OPTIONAL_SPIKE(12) TONFA};
const unsigned short tonfa_labels[21]={2,3,4,5,6,7,8,9,10,11,12,13,14,15,OPTIONAL_SPIKE(206) TONFA_LABEL};
enum { TONFA_ICON = 122 };
static const Byte icons[21]={0x37,0x38,0x39,0x3a,0x3b,0x3c,0x3d,0x3e,0x43,0x45,0x44,0x3f,0x41,0x42,OPTIONAL_SPIKE(121) TONFA_ICON};
Byte tonfa_common_icons[32], tonfa_box_icons[32], tonfa_forge_icons[32];
U tonfa_shop_icons[10];
U tonfa_equip_slots[32], tonfa_actor_classes[32];
static U equip_type(void){return TONFA;}
static U language(void){
    Byte *system=*(Byte **)MAIN(0x188ca48);
    return system ? system[0xa5] : 1;
}
static const char *category(U lang){
    return lang==8 ? "穿龙棍" : lang==7 ? "穿龍棍" : "Tonfa";
}
static int starts(const char *s,const char *word){
    while(*word)if(*s++!=*word++)return 0;
    return 1;
}
static const char *name_for(Byte *row,U level,int index){
    U id=FIELD(U,row,4), lang=language();
    const char *kind=category(lang);
    if (!id || id>FAMILIES || !donor[id]) return kind;
    if (id==DIVINE) return lang==8 ? "神圣双蛇杖·穿龙棍" : lang==7 ? "神聖雙蛇杖·穿龍棍" : "Divine Caduceus";
    typedef const char *(*Name)(Byte *,U);
    const char *src=((Name)(*(U **)donor[id])[index])(donor[id],level);
    if(!src)return kind;
    unsigned slot=0;
    while(slot<3 && cached_names[id][slot] && cached_names[id][slot]!=src)++slot;
    if(slot>=3)slot=2;
    if(cached_names[id][slot]!=src || cached_language[id][slot]!=lang){
        char *s=names[id][slot]; unsigned i=0;
        const char *p=src, *replace=0; unsigned skip=0;
        if(lang==7 || lang==8){
            for(;*p && !replace;++p){
                const char *words[]={"大剑","大劍","巨剑","巨劍","剑","劍","刀","刃"};
                for(unsigned j=0;j<8;++j)if(starts(p,words[j])){
                    replace=p;skip=j<4?6:3;break;
                }
            }
        }
        if(!replace){
            for(const char *k=kind;*k;++k)s[i++]=*k;
            s[i++]=' ';
        }
        for(p=src;*p;){
            if(p==replace){
                if(i+9>71)break;
                for(const char *k=kind;*k;++k)s[i++]=*k;
                p+=skip;
            }else{
                unsigned n=(Byte)*p<128?1:(Byte)*p<224?2:(Byte)*p<240?3:4;
                if(i+n>71)break;
                while(n-- && *p)s[i++]=*p++;
            }
        }
        s[i]=0; cached_names[id][slot]=src; cached_language[id][slot]=lang;
    }
    return names[id][slot];
}
static const char *family_name(Byte *r){return name_for(r,1,0x3c/4);}
static const char *upgrade_name(Byte *r,U level){return name_for(r,level,0xbc/4);}
static const char *description(void){
    U lang=language();
    return lang==8 ? "来自边境的一对穿龙棍。" : lang==7 ? "來自邊境的一對穿龍棍。" : "A pair of Frontier Tonfa.";
}
void module_init(void){
    for(unsigned i=0;i<32;++i){
        tonfa_common_icons[i]=((Byte *)MAIN(0x162e7dc))[i];
        tonfa_box_icons[i]=((Byte *)MAIN(0x164ff08))[i];
        tonfa_forge_icons[i]=((Byte *)MAIN(0x165000c))[i];
    }
    tonfa_common_icons[TONFA]=tonfa_box_icons[TONFA]=tonfa_forge_icons[TONFA]=TONFA_ICON;
    for(unsigned i=0;i<32;++i){
        tonfa_equip_slots[i]=tonfa_actor_classes[i]=255;
        if(i<25){
            tonfa_equip_slots[i]=((U *)MAIN(0x159fd28))[i];
            tonfa_actor_classes[i]=((U *)MAIN(0x159fdcc))[i];
        }
    }
    tonfa_equip_slots[TONFA]=tonfa_equip_slots[18];
    tonfa_actor_classes[TONFA]=tonfa_actor_classes[18];
    for(unsigned i=0;i<10;++i)tonfa_shop_icons[i]=((U *)MAIN(0x176f3e4))[i];
    tonfa_shop_icons[8]=(U)icons;
    logf("tonfa: independent smithy category 16, equipment type 25");
}
int icon_rect_c(U *r){
    if(r[2]!=TONFA_ICON || (r[1]>1 && r[1]!=4))return 0;
    static const float rects[2][4]={{464,64,496,96},{320,896,368,944}};
    float *out=(float *)r[3];
    for(unsigned i=0;i<4;++i)out[i]=rects[r[1]==1][i];
    return 1;
}
static U roster_family(const Byte *gear){
    if(!gear || (FIELD(unsigned short,gear,28)&31)!=TONFA)return 0;
    U family=FIELD(unsigned short,gear,30);
    return family<=FAMILIES ? family : 0;
}
U roster_icon_c(const Byte *gear){
    return roster_family(gear) ? TONFA_ICON : gear[0x4d4];
}
int base_c(U *r){
    if(r[1]!=TONFA)return 0;
    U id=r[2];
    if(id>FAMILIES){r[0]=0;return 1;}
    Byte *base=FN(Getter,0x13c208)((void *)r[0],GS,id==DIVINE ? 1 : id);
    if(!base){r[0]=0;return 1;}
    if(!base_vt[2]){
        U *vt=*(U **)base;
        for(unsigned i=0;i<92;++i)base_vt[i]=(vt-2)[i];
        base_vt[2+0x18/4]=(U)equip_type;
        base_vt[2+0x3c/4]=(U)family_name; base_vt[2+0xbc/4]=(U)upgrade_name;
        base_vt[2+0x40/4]=base_vt[2+0xc0/4]=(U)description;
    }
    for(unsigned i=0;i<10;++i)rows[id][i]=((U *)base)[i];
    donor[id]=base;
    Byte *row=(Byte *)rows[id];
    *(U **)row=base_vt+2;
    if(id){
        FIELD(unsigned short,row,8)=MODEL_FIRST+id-1;
        FIELD(unsigned short,row,10)=47; /* shared paired attachment and draw setup */
    }
    if (id==DIVINE) {
        FIELD(U,row,4)=DIVINE;
        row[13]=1; row[14]=0; row[15]=10;
        FIELD(U,row,16)=13000;
        row[20]=row[21]=0;
        row[26]=0; row[27]=1;
        for(unsigned i=28;i<38;++i)row[i]=0;
        FIELD(unsigned short,row,38)=959;
    }
    r[0]=(U)row;
    return 1;
}
/* The original balance uses native GS levels with scaled attack and element. */
static U regular_levels[959][5];
static U divine_level[5];
static U divine_level_vtable[92];
static U divine_sharpness(void *row,U colour){
    (void)row;
    static const unsigned short ends[7]={0,0,0,0,0,280,450};
    return colour<7 ? ends[colour] : 0;
}
static struct {
    U vtable;
    unsigned short level, family, display, item[4];
    Byte quantity[4], key[4];
    unsigned short category, rank, scrap[3], scrap_count[3], pad;
} divine_recipe;
static U divine_process[9];
int level_c(U *r){
    if(r[1]!=TONFA)return 0;
    U id=r[2];
    if(id>959){r[0]=0;return 1;}
    Byte *src=FN(Getter,0x13ed08)((void *)r[0],GS,id==959 ? 1 : id);
    if(!src){r[0]=0;return 1;}
    if(id<959){
        U *row=regular_levels[id];
        for(unsigned i=0;i<5;++i)row[i]=((U *)src)[i];
        FIELD(unsigned short,row,12)=(FIELD(unsigned short,src,12)*RAW_PERCENT+50)/100;
        FIELD(unsigned short,row,16)=(FIELD(unsigned short,src,16)*ELEMENT_PERCENT+50)/100;
        r[0]=(U)row;return 1;
    }
    Byte *dst=(Byte *)divine_level;
    if(!divine_level[0]){
        const U *vt=*(U **)src;
        for(unsigned i=0;i<92;++i)divine_level_vtable[i]=(vt-2)[i];
        divine_level_vtable[2+0x80/4]=(U)divine_sharpness;
        divine_level[0]=(U)(divine_level_vtable+2);
    }
    divine_level[1]=959;
    dst[8]=DIVINE; dst[9]=1; dst[10]=44; dst[11]=10;
    FIELD(unsigned short,dst,12)=330; dst[15]=5;
    r[0]=(U)dst;return 1;
}
int create_c(U *r){
    if(r[1]!=TONFA)return 0;
    if(r[2]<95){r[1]=GS;return 0;}
    if(r[2]!=95){r[0]=0;return 1;}
    divine_recipe.vtable=MAIN(0x1768234);
    divine_recipe.level=1; divine_recipe.family=DIVINE;
    divine_recipe.item[0]=1553; divine_recipe.quantity[0]=10; divine_recipe.key[0]=2;
    r[0]=(U)&divine_recipe;return 1;
}
int process_c(U *r){
    if(r[1]!=TONFA)return 0;
    if(r[2]<958){r[1]=GS;return 0;}
    if(r[2]!=958){r[0]=0;return 1;}
    divine_process[0]=MAIN(0x17682e4); divine_process[1]=(1u<<16)|DIVINE;
    r[0]=(U)divine_process;return 1;
}

/* The native DB file-ID range ends before these model numbers. Look up the
   two private model resources explicitly, keeping both hands independent. */
enum { FIRST_FILE = 0x11800 };
int model_file_c(U *r){
    unsigned family=r[3]-MODEL_FIRST;
    if(r[1]!=11 || family>=FAMILIES || r[2]>1)return 0;
    r[0]=FIRST_FILE+2*family+r[2];return 1;
}
static U path_hash(const char *p){
    U crc=~0u;
    while(*p){
        crc^=(Byte)*p++;
        for(unsigned i=0;i<8;++i)crc=(crc>>1)^(0xedb88320u&-(crc&1u));
    }
    return crc;
}
int file_resource_c(U *r){
    unsigned file=r[1]-FIRST_FILE;
    if(file>=2*FAMILIES)return 0;
    unsigned model=MODEL_FIRST+file/2;
    char name[]="weapon\\w11\\sou000\\sou_r000";
    for(unsigned i=0;i<3;++i){
        char digit='0'+model%10;model/=10;
        name[16-i]=digit;name[sizeof(name)-2-i]=digit;
    }
    if(file&1)name[sizeof(name)-5]='l';
    void *manager=*(void **)MAIN(0x2126a64);
    if(!manager){r[0]=0;return 1;}
    typedef void *(*Find)(void *,U,U,U);
    Find find=(*(Find **)manager)[0x38/4];
    r[0]=(U)find(manager,(U)find,path_hash(name),0x58a15856);
    logf("tonfa model %s -> %x",name,r[0]);return 1;
}
static int equipment_messages(const char *name){
    const char *target="LbShop_EqMakeMsg";if(!name)return 0;
    while(*target)if(*target++!=*name++)return 0;
    return *name==0 || *name=='_';
}
int text_c(U *r){
    Byte *m=(Byte *)r[4];
    if(!m || FIELD(U,m,0x88)!=206 || !equipment_messages(FIELD(const char *,m,0x8c)))return 0;
    const char **old=FIELD(const char **,m,0x98);
    typedef void *(*Alloc)(void *,U);
    const char **arr=((Alloc)(*(U **)m)[0x40/4])(m,(TONFA_LABEL+1)*4);
    if(!arr || !old)return 0;
    for(unsigned i=0;i<206;++i)arr[i]=old[i];
#if WITH_MAGNET
    arr[206]="Magnet Spike";
#endif
    arr[TONFA_LABEL]=category(language());
    FIELD(const char **,m,0x98)=arr;FIELD(U,m,0x88)=TONFA_LABEL+1;
    logf("tonfa: registered forge page three labels");
    return 0;
}
