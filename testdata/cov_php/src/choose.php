<?php
declare(strict_types=1);

function choose(int $n): string {
    if ($n > 0) {
        return 'pos';
    }
    return 'neg';
}
