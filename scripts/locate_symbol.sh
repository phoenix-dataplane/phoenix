#!/usr/bin/env bash

for lib in `find target -type f -name "*.rlib"`; do
	echo $lib;
	# nm $lib | grep '_ZN86_$LT$libc..unix..linux_like..linux..gnu..b64..sigset_t$u20$as$u20$core..fmt..Debug$GT$3fmt17h43c84351e16aca3cE';
	nm $lib | grep '__rust_probestack' --color
done
