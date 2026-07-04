" date_format.vim — the date/time builtins (time.c: strftime formats an epoch
" second into a local-time string, strptime parses one back into an epoch).
" Both use the process timezone, so an ABSOLUTE assert (strftime of a fixed
" epoch) would vary by machine; every case here is a strptime->strftime
" round-trip whose local-offset cancels, making the result TZ-independent.
" Self-test: asserts into v:errors, throws if any failed.

" --- full date+time round-trips through a matching format string
call assert_equal('2020-06-15 12:30:45', strftime('%Y-%m-%d %H:%M:%S', strptime('%Y-%m-%d %H:%M:%S', '2020-06-15 12:30:45')))
call assert_equal('1999/12/31', strftime('%Y/%m/%d', strptime('%Y/%m/%d', '1999/12/31')))
call assert_equal('01-01-2000', strftime('%d-%m-%Y', strptime('%d-%m-%Y', '01-01-2000')))

" --- leap day survives the round-trip (2016 is a leap year)
call assert_equal('2016-02-29', strftime('%Y-%m-%d', strptime('%Y-%m-%d', '2016-02-29')))

" --- reformatting: parse a full stamp, emit only part of it (offset cancels)
call assert_equal('08:05', strftime('%H:%M', strptime('%H:%M %Y-%m-%d', '08:05 2021-03-10')))

" --- literal '%%' and an empty format carry no time component -> TZ-safe
call assert_equal('%', strftime('%%', 0))
call assert_equal('', strftime('', 0))

" --- types: strftime yields a String, strptime yields a Number
call assert_equal(v:t_string, type(strftime('%Y', 0)))
call assert_equal(v:t_number, type(strptime('%Y', '2020')))

" --- strptime of a known stamp is a positive epoch (post-1970)
call assert_equal(1, strptime('%Y-%m-%d %H:%M:%S', '2001-09-09 01:46:40') > 0)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'date_format.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'date_format.vim: all assertions passed'
