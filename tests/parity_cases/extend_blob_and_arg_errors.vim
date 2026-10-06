" extend()/extendnew() of a Blob (vim 9.2 blob_extend_func) and the vim 9.2
" argument errors of extend(), insert(), items().
echo extend(0z01, 0z0203) extend(0z01, 0z0203, 0) extend(0z01, 0z02, 1) extendnew(0z01, 0z02)
echo extend(0z01, 0z02, 5)
echo extend(0z01, [1])
echo extend([1], 0z01)
echo extendnew(1, 2)
echo insert('a', 1)
echo items(1.5)
let b = 0z01 | lockvar b
echo extend(b, 0z02)
echo 'end'
echo string(add(v:null, 1))
echo string(insert(1, 2))
echo string(remove('abc', 1))
echo string(keys(1)) string(values(v:null))
echo string(items(1))
echo string(copy(function('len')))
echo string(count(1, 1))
echo string(index('abc', 'b'))
echo string(extend(1, 2))
echo string(extend([1], {}))
echo string(filter(1, 'v:val'))
echo string(map(v:null, 'v:val'))
