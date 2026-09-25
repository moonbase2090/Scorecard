<?php
declare(strict_types=1);

use PHPUnit\Framework\TestCase;

final class ChooseTest extends TestCase {
    public function testPositive(): void {
        require_once __DIR__ . '/../src/choose.php';
        $this->assertSame('pos', choose(1));
    }
}
