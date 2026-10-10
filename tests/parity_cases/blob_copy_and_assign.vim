" copy()/deepcopy() of a Blob are independent buffers; a subscript or range
" assignment follows set_var_lval: i == len appends, a negative subscript is
" E979 as written, a range needs idx1 <= len and idx2 inside the blob.
let b = 0z0102FF
let c = copy(b)
let c[1] = 0x41
echo c b
let d = deepcopy(b)
let d[0] = 9
echo b d
let e = b
let e[0] = 7
echo b
let c = 0z010203
let c[3] = 7
echo c
let c[5] = 7
let c[-1] = 7
let c[0] = 256
let c[0] = -1
let c[0] = []
echo c
let c[1:1] = 0z99
let c[0:1] = 0z00
let c[2:3] = 0z0000
let c[3:3] = 0z01
let c[-1:-1] = 0z42
let c[1:0] = 0z
let c[0:0] = 5
let c[5:9] = 0z01
let c[3:2] = 0z01
let c[9:2] = 0z01
echo c
let c[1:] = 0z7788
let c[:0] = 0z55
echo c
let c[3:] = 0z01
echo c
