package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"fmt"
	"os"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/aws-sdk-go-v2/service/s3/types"
)

func main() {
	ctx := context.Background()
	endpoint := os.Getenv("S3_ENDPOINT_DOCKER")
	bucket := os.Getenv("S3_BUCKET")
	region := os.Getenv("S3_REGION")
	if region == "" {
		region = "us-east-1"
	}
	body := []byte("suite-b-go\n")
	cfg, err := config.LoadDefaultConfig(ctx,
		config.WithRegion(region),
		config.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(
			os.Getenv("S3_ACCESS_KEY_ID"),
			os.Getenv("S3_SECRET_ACCESS_KEY"),
			"",
		)),
	)
	if err != nil {
		panic(err)
	}
	client := s3.NewFromConfig(cfg, func(o *s3.Options) {
		o.BaseEndpoint = aws.String(endpoint)
		o.UsePathStyle = true
		o.RequestChecksumCalculation = aws.RequestChecksumCalculationWhenRequired
		o.ResponseChecksumValidation = aws.ResponseChecksumValidationWhenRequired
	})
	_, err = client.CreateBucket(ctx, &s3.CreateBucketInput{Bucket: aws.String(bucket)})
	if err != nil {
		fmt.Println("create-bucket:", err)
	}
	_, err = client.PutObject(ctx, &s3.PutObjectInput{
		Bucket: aws.String(bucket),
		Key:    aws.String("go/hello.txt"),
		Body:   bytes.NewReader(body),
	})
	if err != nil {
		panic(err)
	}
	out, err := client.GetObject(ctx, &s3.GetObjectInput{
		Bucket: aws.String(bucket),
		Key:    aws.String("go/hello.txt"),
	})
	if err != nil {
		panic(err)
	}
	defer out.Body.Close()
	buf := new(bytes.Buffer)
	if _, err = buf.ReadFrom(out.Body); err != nil {
		panic(err)
	}
	if sha256.Sum256(buf.Bytes()) != sha256.Sum256(body) {
		panic(fmt.Sprintf("sha256 mismatch %q", buf.Bytes()))
	}
	_, _ = client.DeleteObject(ctx, &s3.DeleteObjectInput{
		Bucket: aws.String(bucket),
		Key:    aws.String("go/hello.txt"),
	})
	_ = types.Object{}
	fmt.Println("aws-sdk-go-v2 sha256 ok")
}
