// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include <boost/filesystem.hpp>
#include <gtest/gtest.h>
#include <chrono>
#include <fstream>
#include <ostream>
#include <string>
#include <thread>

#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "testinfra/if/gen-cpp2/test_result_artifacts_types.h"

TEST(SimpleTest, artifact_test) {
  char* artifacts_dir = std::getenv("TEST_RESULT_ARTIFACTS_DIR");

  if (artifacts_dir) {
    boost::filesystem::create_directory(artifacts_dir);

    std::string dummy_log = std::string(artifacts_dir) + "/dummy_log.txt";
    std::ofstream file(dummy_log.c_str());
    file << "Hello world!\n";
    file.close();
  }
}

TEST(SimpleTest, artifact_with_annotation_test) {
  char* artifacts_dir = std::getenv("TEST_RESULT_ARTIFACTS_DIR");
  char* annotation_dir = std::getenv("TEST_RESULT_ARTIFACT_ANNOTATIONS_DIR");

  if (artifacts_dir) {
    boost::filesystem::create_directory(artifacts_dir);

    std::string dummy_log = std::string(artifacts_dir) + "/dummy_log.txt";
    std::ofstream log_file(dummy_log.c_str());
    log_file << "Hello from dummy log!\n";
    log_file.close();

    std::string dummy_blob = std::string(artifacts_dir) + "/dummy_blob.txt";
    std::ofstream blob_file(dummy_blob.c_str());
    blob_file << "Hello from dummy blob!\n";
    blob_file.close();
  }

  if (annotation_dir) {
    boost::filesystem::create_directory(annotation_dir);
    std::string annotation_file =
        std::string(annotation_dir) + "/dummy_log.txt.annotation";

    std::ofstream annotationFile(annotation_file);
    facebook::testinfra::artifacts::TestResultArtifactAnnotations
        log_annotation;
    facebook::testinfra::artifacts::TestArtifactType artifact_type;
    artifact_type.set_generic_text_log() =
        facebook::testinfra::artifacts::GenericTextLog();
    log_annotation.type() = artifact_type;
    annotationFile
        << apache::thrift::SimpleJSONSerializer::serialize<std::string>(
               log_annotation);
    annotationFile.close();
  }

  if (std::getenv("TPX_PLAYGROUND_SLEEP")) {
    int i = std::stoi(std::getenv("TPX_PLAYGROUND_SLEEP"));
    auto duration = std::chrono::seconds(i);
    // This sleep is intentional, we want to test target to timeout on
    // request.
    //
    // NOLINTNEXTLINE(facebook-hte-BadCall-sleep_for)
    std::this_thread::sleep_for(duration);
  }
}
