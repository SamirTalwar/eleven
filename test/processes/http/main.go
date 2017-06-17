package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"os"
)

type Configuration struct {
	Port    int    `json:"port"`
	Process string `json:"process"`
}

type HttpRequest struct {
	Method string `json:"method"`
	Path   string `json:"path"`
}

type HttpResponse struct {
	Status int    `json:"status"`
	Body   string `json:"body"`
}

func main() {
	configuration := readConfiguration(os.Args[2])

	http.HandleFunc("/", func(response http.ResponseWriter, request *http.Request) {
		requestStruct := HttpRequest{Method: request.Method, Path: request.URL.Path}
		var responseStruct HttpResponse
		bufferedResponse := bufio.NewWriter(response)

		socket, err := net.Dial("unix", configuration.Process)
		if err != nil {
			panic(err)
		}
		defer socket.Close()

		err = json.NewEncoder(socket).Encode(requestStruct)
		if err != nil {
			panic(err)
		}
		if v, ok := socket.(interface {
			CloseWrite() error
		}); ok {
			v.CloseWrite()
		}

		err = json.NewDecoder(socket).Decode(&responseStruct)
		if err != nil {
			response.WriteHeader(500)
			_, err = bufferedResponse.WriteString(err.Error())
			if err != nil {
				panic(err)
			}
		} else {
			response.WriteHeader(responseStruct.Status)
			_, err = bufferedResponse.WriteString(responseStruct.Body)
			if err != nil {
				panic(err)
			}
		}
		bufferedResponse.Flush()
	})
	http.ListenAndServe(fmt.Sprintf(":%d", configuration.Port), nil)
}

func readConfiguration(file string) Configuration {
	configurationFile, err := os.Open(file)
	configurationFileReader := bufio.NewReader(configurationFile)
	if err != nil {
		panic(err)
	}

	var configuration Configuration
	decoder := json.NewDecoder(configurationFileReader)
	err = decoder.Decode(&configuration)
	configurationFile.Close()
	if err != nil {
		panic(err)
	}

	return configuration
}
