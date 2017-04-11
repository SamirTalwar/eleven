package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"os/signal"
	"strings"
	"syscall"
)

type Config struct {
	Routes []Route `json:"routes"`
}

type Route struct {
	Method  string `json:"method"`
	Path    string `json:"path"`
	Process string `json:"process"`
}

type Request struct {
	Method string `json:"method"`
	Path   string `json:"path"`
}

type Response struct {
	Status int    `json:"status"`
	Body   string `json:"body"`
}

func main() {
	config := readConfig(os.Args[1])

	running := true

	signalChannel := make(chan os.Signal, 2)
	signal.Notify(signalChannel, os.Interrupt, syscall.SIGTERM)
	go func() {
		<-signalChannel
		running = false
	}()

	inputReader := bufio.NewReader(os.Stdin)
	input := json.NewDecoder(inputReader)
	outputWriter := bufio.NewWriter(os.Stdout)
	output := json.NewEncoder(outputWriter)

	for running {
		var request Request
		var response Response

		err := input.Decode(&request)
		if err == io.EOF {
			break
		}
		if err != nil {
			panic(err)
		}

		fmt.Fprintf(os.Stderr, "Request: %+v\n", request)

		process := ""
		for _, route := range config.Routes {
			if request.Method == strings.ToUpper(route.Method) && request.Path == route.Path {
				process = route.Process
			}
		}

		if process == "" {
			fmt.Fprintf(os.Stderr, "Request discarded.\n")
			response = Response{Status: 404, Body: ""}
		} else {
			socket, err := net.Dial("unix", process)
			if err != nil {
				panic(err)
			}

			err = json.NewEncoder(socket).Encode(request)
			if err != nil {
				panic(err)
			}
			err = json.NewDecoder(socket).Decode(&response)
			if err != nil {
				panic(err)
			}
		}

		fmt.Fprintf(os.Stderr, "Response: %+v\n", response)

		err = output.Encode(response)
		outputWriter.Flush()
		if err != nil {
			panic(err)
		}
	}
}

func readConfig(file string) Config {
	configFile, err := os.Open(file)
	configFileReader := bufio.NewReader(configFile)
	if err != nil {
		panic(err)
	}

	var config Config
	decoder := json.NewDecoder(configFileReader)
	err = decoder.Decode(&config)
	configFile.Close()
	if err != nil {
		panic(err)
	}

	return config
}
