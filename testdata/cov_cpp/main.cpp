#include <cassert>
#include <string>

std::string choose(int n) {
  if (n > 0) {
    return "pos";
  }
  return "neg";
}

int main() {
  assert(choose(1) == "pos");
  return 0;
}
