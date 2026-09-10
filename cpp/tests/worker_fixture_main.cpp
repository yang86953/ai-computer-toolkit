#include <windows.h>

#include <iostream>
#include <string>

int main() {
    std::string request;
    std::getline(std::cin, request);
    Sleep(10000U);
    std::cout << "{}\n";
    return 0;
}
