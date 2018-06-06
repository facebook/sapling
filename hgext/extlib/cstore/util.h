template <class T>
void removeFromVector(std::vector<T>& vec, T& item) {
  for (auto it = vec.begin(); it != vec.end(); ++it) {
    if (*it == item) {
      vec.erase(it);
      break;
    }
  }
}
